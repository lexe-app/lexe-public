use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{notify, shutdown::ShutdownChannel, task::LxTask};
use tokio::{
    sync::{mpsc, oneshot},
    time::{interval, interval_at, Instant},
};
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};

use crate::{
    alias::{LexeChainMonitorType, P2PGossipSyncType, ProbabilisticScorerType},
    traits::{
        LexeChannelManager, LexeEventHandler, LexeInnerPersister,
        LexePeerManager, LexePersister,
    },
};

// If LDK's `get_event_or_persistence_needed_future` future is failing to wake
// the BGP, this timer can be reduced to say ~3s in prod to ensure events are
// handled. See the original comment here (via blame) for more info.
const PROCESS_EVENTS_INTERVAL: Duration = Duration::from_secs(60);
const PEER_MANAGER_TICK_INTERVAL: Duration = Duration::from_secs(15);
const CHANNEL_MANAGER_TICK_INTERVAL: Duration = Duration::from_secs(60);
const NETWORK_GRAPH_INITIAL_DELAY: Duration = Duration::from_secs(60);
const NETWORK_GRAPH_PRUNE_INTERVAL: Duration = Duration::from_secs(15 * 60);
const PROB_SCORER_PERSIST_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// A Tokio-native background processor that runs on a single task and does not
/// spawn any OS threads. Modeled after the lightning-background-processor crate
/// provided by LDK - see that crate's implementation for more details.
pub struct LexeBackgroundProcessor {}

impl LexeBackgroundProcessor {
    #[instrument(skip_all, name = "(bgp)")]
    pub fn start<CM, PM, PS, EH>(
        channel_manager: CM,
        peer_manager: PM,
        persister: PS,
        chain_monitor: Arc<LexeChainMonitorType<PS>>,
        event_handler: EH,
        gossip_sync: Arc<P2PGossipSyncType>,
        scorer: Arc<Mutex<ProbabilisticScorerType>>,
        // TODO(max): A `process_events` notification should be sent every time
        // an event is generated which does not also cause the future returned
        // by `get_event_or_persistence_needed_future()` to resolve.
        //
        // Ideally, we can remove this channel entirely, but a manual trigger
        // is currently still required after every channel monitor
        // persist (which may resume monitor updating and create more
        // events). This was supposed to be resolved by LDK#2052 and
        // LDK#2090, but our integration tests still fail without this channel.
        mut process_events_rx: mpsc::Receiver<oneshot::Sender<()>>,
        mut scorer_persist_rx: notify::Receiver,
        mut shutdown: ShutdownChannel,
    ) -> LxTask<()>
    where
        CM: LexeChannelManager<PS>,
        PM: LexePeerManager<CM, PS>,
        PS: LexePersister,
        EH: LexeEventHandler,
    {
        LxTask::spawn_named("background processor", async move {
            let mut process_events_timer = interval(PROCESS_EVENTS_INTERVAL);
            let mut pm_timer = interval(PEER_MANAGER_TICK_INTERVAL);
            let mut cm_tick_timer = interval(CHANNEL_MANAGER_TICK_INTERVAL);
            let start = Instant::now() + NETWORK_GRAPH_INITIAL_DELAY;
            let mut ng_timer = interval_at(start, NETWORK_GRAPH_PRUNE_INTERVAL);
            let mut scorer_timer = interval(PROB_SCORER_PERSIST_INTERVAL);

            // This is the event handler future generator type required by LDK
            let mk_event_handler_fut =
                |event| event_handler.get_ldk_handler_future(event);

            loop {
                // A future that completes when any of the following applies:
                //
                // 1) We need to reprocess events
                // 2) The channel manager got an update (event or repersist)
                // 3) The chain monitor got an update
                // 4) The background processor was explicitly triggered
                let mut processed_txs = Vec::new();
                let process_events_fut = async {
                    tokio::select! {
                        biased;
                        () = channel_manager
                            .get_event_or_persistence_needed_future() =>
                            debug!("Triggered: Channel manager update"),
                        () = chain_monitor.get_update_future() =>
                            debug!("Triggered: Chain monitor update"),
                        _ = process_events_timer.tick() =>
                            debug!("Triggered: process_events_timer ticked"),
                        // TODO(max): If LDK has fixed the BGP waking issue,
                        // our integration tests should pass with this branch
                        // commented out.
                        Some(tx) = process_events_rx.recv() => {
                            debug!("Triggered: process_events channel");
                            processed_txs.push(tx);
                        }
                    };

                    // We're about to process events. Prevent duplicate work by
                    // resetting the process_events_timer & clearing out the
                    // process_events channel.
                    process_events_timer.reset();
                    while let Ok(tx) = process_events_rx.try_recv() {
                        processed_txs.push(tx);
                    }
                };

                // A future that completes when the scorer persist timer ticks
                // or a notification is sent over the `scorer_persist` channel.
                let scorer_persist_fut = async {
                    tokio::select! {
                        _ = scorer_timer.tick() =>
                            debug!("Triggered: Scorer persist timer"),
                        () = scorer_persist_rx.recv() => {
                            debug!("Triggered: Scorer persist channel");
                            scorer_timer.reset();
                        }
                    }
                };

                tokio::select! {
                    // --- Process events + channel manager repersist --- //
                    () = process_events_fut => {
                        debug!("Processing pending events");

                        channel_manager
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(event-handler)(chan-man)"))
                            .await;
                        chain_monitor
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(event-handler)(chain-mon)"))
                            .await;

                        async {
                            peer_manager.process_events();
                        }.instrument(info_span!("(process)(peer-man)")).await;

                        // Notify waiters that events have been processed.
                        for tx in processed_txs {
                            let _ = tx.send(());
                        }

                        if channel_manager.get_and_clear_needs_persistence() {
                            let try_persist = persister
                                .persist_manager(channel_manager.deref())
                                .await;
                            if let Err(e) = try_persist {
                                // Failing to persist the channel manager won't
                                // lose funds so long as the chain monitors have
                                // been persisted correctly, but it's still
                                // serious - initiate a shutdown
                                error!("Channel manager persist error: {e:#}");
                                break shutdown.send();
                            }
                        }
                    }

                    // --- Scorer persist branch --- //
                    () = scorer_persist_fut => {
                        debug!("Persisting probabilistic scorer");
                        let persist_res = persister
                            .persist_scorer(scorer.as_ref())
                            .await;
                        if let Err(e) = persist_res {
                            // The scorer isn't super important.
                            warn!("Couldn't persist scorer: {e:#}");
                        }
                    }

                    // --- Timer tick branches --- //
                    _ = pm_timer.tick() => {
                        debug!("Calling PeerManager::timer_tick_occurred()");
                        peer_manager.timer_tick_occurred();
                    }
                    _ = cm_tick_timer.tick() => {
                        debug!("Calling ChannelManager::timer_tick_occurred()");
                        channel_manager.timer_tick_occurred();
                    }
                    _ = ng_timer.tick() => {
                        debug!("Pruning and persisting network graph");
                        // TODO(max): Don't prune during RGS. See LDK's BGP.
                        // Relevant after we've implemented RGS.
                        let network_graph = gossip_sync.network_graph();
                        network_graph.remove_stale_channels_and_tracking();
                        let persist_res = persister
                            .persist_graph(network_graph)
                            .await;
                        if let Err(e) = persist_res {
                            // The network graph isn't super important.
                            warn!("Couldn't persist network graph: {:#}", e);
                        }
                    }

                    // --- Shutdown branch --- //
                    () = shutdown.recv() => {
                        break info!("Background processor shutting down");
                    }
                }
            }

            // Persist everything one last time.
            // - For the channel manager, this may prevent some races where the
            //   node quits while channel updates were in-flight, causing
            //   ChannelMonitor updates to be persisted without corresponding
            //   ChannelManager updating being persisted. This does not risk the
            //   loss of funds, but upon next boot the ChannelManager may
            //   accidentally trigger a force close..
            // - For the network graph and scorer, it is possible that the node
            //   is shut down before they have gotten a chance to be persisted,
            //   (e.g. `shutdown_after_sync` is set), and since we're already
            //   another API call for the channel manager, we might as well
            //   concurrently persist these as well.
            let network_graph = gossip_sync.network_graph();
            let results = tokio::join!(
                persister.persist_manager(channel_manager.deref()),
                persister.persist_graph(network_graph),
                persister.persist_scorer(scorer.as_ref()),
            );
            for res in <[_; 3]>::from(results) {
                if let Err(e) = res {
                    error!("Final persistence failure: {e:#}");
                }
            }
        })
    }
}
