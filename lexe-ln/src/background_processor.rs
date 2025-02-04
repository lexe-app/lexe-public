use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{notify, notify_once::NotifyOnce, task::LxTask};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::{
    alias::{LexeChainMonitorType, P2PGossipSyncType, ProbabilisticScorerType},
    traits::{
        LexeChannelManager, LexeEventHandler, LexeInnerPersister,
        LexePeerManager, LexePersister,
    },
};

/// The intervals for the timers used in the BGP.
mod interval {
    use std::time::Duration;

    /// Channel manager ticks.
    pub const CHANNEL_MANAGER: Duration = Duration::from_secs(60);
    /// Network graph prunes.
    pub const NETWORK_GRAPH: Duration = Duration::from_secs(15 * 60);
    /// Peer manager ticks.
    pub const PEER_MANAGER: Duration = Duration::from_secs(15);
    /// Probabilistic scorer persists.
    pub const PROB_SCORER: Duration = Duration::from_secs(5 * 60);
    /// Event processing.
    // If LDK's `get_event_or_persistence_needed_future` future is failing to
    // wake the BGP, this timer can be reduced to say ~3s in prod to ensure
    // events are handled. process_events_tx can also be used.
    pub const PROCESS_EVENTS: Duration = Duration::from_secs(60);
}

/// The initial delays for the timers used in the BGP.
mod delay {
    use std::time::Duration;

    pub const CHANNEL_MANAGER: Duration = Duration::from_millis(0);
    pub const NETWORK_GRAPH: Duration = Duration::from_millis(200);
    pub const PEER_MANAGER: Duration = Duration::from_millis(400);
    pub const PROB_SCORER: Duration = Duration::from_millis(600);
    pub const PROCESS_EVENTS: Duration = Duration::from_millis(800);
}

/// A Tokio-native background processor that runs on a single task and does not
/// spawn any OS threads. Modeled after the lightning-background-processor crate
/// provided by LDK - see that crate's implementation for more details.
pub fn start<CM, PM, PS, EH>(
    channel_manager: CM,
    peer_manager: PM,
    persister: PS,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    event_handler: EH,
    gossip_sync: Arc<P2PGossipSyncType>,
    scorer: Arc<Mutex<ProbabilisticScorerType>>,
    // Whether to persist the network graph.
    persist_graph: bool,
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
    mut shutdown: NotifyOnce,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
    EH: LexeEventHandler,
{
    LxTask::spawn_with_span(
        "background processor",
        info_span!("(bgp)"),
        async move {
            let now = Instant::now();

            let mk_interval = |delay: Duration, interval: Duration| {
                // Remove the staggering in debug mode in an attempt to catch
                // any subtle race conditions which may arise
                let start = if cfg!(debug_assertions) {
                    now
                } else {
                    now + delay
                };
                tokio::time::interval_at(start, interval)
            };

            let mut process_events_timer =
                mk_interval(delay::PROCESS_EVENTS, interval::PROCESS_EVENTS);
            let mut pm_timer =
                mk_interval(delay::PEER_MANAGER, interval::PEER_MANAGER);
            let mut cm_timer =
                mk_interval(delay::CHANNEL_MANAGER, interval::CHANNEL_MANAGER);
            let mut ng_timer =
                mk_interval(delay::NETWORK_GRAPH, interval::NETWORK_GRAPH);
            let mut scorer_timer =
                mk_interval(delay::PROB_SCORER, interval::PROB_SCORER);

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

                        // NOTE(phlip9): worried the `Connection` ->
                        // `process_events` flow might starve the BGP if it
                        // grabs the `process_events` lock and is forced to do
                        // a neverending amount of work under load.
                        //
                        // TODO(phlip9): Consider sending a notification to the
                        // new `process_events` task and waiting for that to
                        // complete?
                        async {
                            peer_manager.process_events();
                        }.instrument(info_span!("(process-bgp)(peer-man)")).await;

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
                    _ = cm_timer.tick() => {
                        debug!("Calling ChannelManager::timer_tick_occurred()");
                        channel_manager.timer_tick_occurred();
                    }
                    _ = ng_timer.tick() => {
                        if persist_graph {
                            debug!("Pruning and persisting network graph");
                            let network_graph = gossip_sync.network_graph();
                            // TODO(max): Don't prune during RGS. See LDK's BGP.
                            // Relevant after we've implemented RGS.
                            network_graph.remove_stale_channels_and_tracking();
                            let persist_res = persister
                                .persist_graph(network_graph)
                                .await;
                            if let Err(e) = persist_res {
                                // The network graph isn't super important.
                                warn!("Couldn't persist network graph: {:#}", e);
                            }
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
        },
    )
}
