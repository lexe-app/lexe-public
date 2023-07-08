use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use common::{shutdown::ShutdownChannel, task::LxTask};
use lightning::events::EventsProvider;
use tokio::{
    sync::{mpsc, oneshot},
    time::{interval, interval_at, Instant},
};
use tracing::{
    debug, error, info, info_span, instrument, trace, warn, Instrument,
};

use crate::{
    alias::{LexeChainMonitorType, P2PGossipSyncType, ProbabilisticScorerType},
    traits::{
        LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
    },
};

// Since the BGP relies on LDK's waker system which has historically been the
// source for a lot of subtle and hard-to-debug bugs, we want to use a
// relatively frequent `PROCESS_EVENTS_INTERVAL` of 3 seconds when running in
// production, to mitigate any bugs which may have slipped through our
// integration tests. What we really want, however, is to remove this timer
// entirely, in order to maximize the amount of time that nodes spend sleeping.
// However, this is blocked on several things:
//
// 1) LDK doesn't support this yet; i.e. we are waiting on an LDK-provided
//    future which resolves immediately after any event is made available for
//    processing: https://github.com/lightningdevkit/rust-lightning/issues/2052
// 2) Until we have extensively tested the new future exposed in (1), we cannot
//    rely on it, and thus need the 3 second interval as a fallback. In our
//    debug builds and tests, however, we will use a much more infrequent 60
//    second timer in order to surface more bugs caused by unprocessed events.
// 3) So long as LDK's BGP still has a 100ms timer, LDK itself has not signalled
//    confidence in the future that they will provide in (1). So long as this is
//    the case, we should keep `PROCESS_EVENTS_INTERVAL` around as a backup.
#[cfg(debug_assertions)]
const PROCESS_EVENTS_INTERVAL: Duration = Duration::from_secs(60);
#[cfg(not(debug_assertions))]
const PROCESS_EVENTS_INTERVAL: Duration = Duration::from_secs(3);
const PEER_MANAGER_PING_INTERVAL: Duration = Duration::from_secs(15);
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
        // A `process_events` notification should be sent every time an event
        // is generated which does not also cause
        // get_persistable_update_future() to resolve. Currently, we only need
        // to do this after a channel monitor persist is successfully completed
        // (which may resume monitor updating / broadcast a funding tx). We may
        // be able to get rid of this once LDK#2052 is implemented. See the
        // comment above `PROCESS_EVENTS_INTERVAL` for more info.
        mut process_events_rx: mpsc::Receiver<oneshot::Sender<()>>,
        // If any events produced a fatal error (`EventHandleError::Fatal`),
        // the event handler will notify us via this bool. It is the BGP's
        // responsibility to ensure that events are not lost by preventing the
        // channel manager and other event providers from being repersisted.
        fatal_event: Arc<AtomicBool>,
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
            let mut pm_timer = interval(PEER_MANAGER_PING_INTERVAL);
            let mut cm_tick_timer = interval(CHANNEL_MANAGER_TICK_INTERVAL);
            let start = Instant::now() + NETWORK_GRAPH_INITIAL_DELAY;
            let mut ng_timer = interval_at(start, NETWORK_GRAPH_PRUNE_INTERVAL);
            let mut ps_timer = interval(PROB_SCORER_PERSIST_INTERVAL);

            loop {
                trace!("Beginning BGP loop iteration");

                // A future that completes whenever we need to reprocess events.
                // Returns a bool indicating whether we also need to repersist
                // the channel manager. NOTE: The "channel manager update"
                // branch is intentionally included here because LDK's
                // background processor always processes events just prior to
                // any channel manager persist.
                let mut processed_txs = Vec::new();
                let process_events_fut = async {
                    let repersist_channel_manager = tokio::select! {
                        biased;
                        () = channel_manager.get_persistable_update_future() => {
                            debug!("Channel manager got persistable update");
                            true
                        }
                        _ = process_events_timer.tick() => {
                            debug!("process_events_timer ticked");
                            false
                        }
                        Some(tx) = process_events_rx.recv() => {
                            debug!("Triggered by process_events channel");
                            processed_txs.push(tx);
                            false
                        }
                    };

                    // We're about to process events. Prevent duplicate work by
                    // resetting the process_events_timer & clearing out the
                    // process_events channel.
                    process_events_timer.reset();
                    while let Ok(tx) = process_events_rx.try_recv() {
                        processed_txs.push(tx);
                    }

                    repersist_channel_manager
                };

                tokio::select! {
                    // --- Process events + channel manager repersist --- //
                    repersist_channel_manager = process_events_fut => {
                        debug!("Processing pending events");
                        // TODO(max): These async blocks can be removed once we
                        // switch to async event handling.
                        async {
                            channel_manager
                                .process_pending_events(&event_handler);
                        }.instrument(info_span!("(process)(chan-man)")).await;
                        async {
                            chain_monitor
                                .process_pending_events(&event_handler);
                        }.instrument(info_span!("(process)(chain-mon)")).await;

                        // If there was a fatal error, exit here before (1) any
                        // messages are sent by the peer manager; (2) anything
                        // is repersisted, especially the channel manager.
                        // `return` instead of `break` also skips the final
                        // repersist at the end of the run body.
                        if fatal_event.load(Ordering::Acquire) {
                            return;
                        }

                        async {
                            peer_manager.process_events();
                        }.instrument(info_span!("(process)(peer-man)")).await;

                        // Notify waiters that events have been processed.
                        for tx in processed_txs {
                            let _ = tx.send(());
                        }

                        if repersist_channel_manager {
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

                    // --- Timer tick branches --- //
                    _ = pm_timer.tick() => {
                        debug!("Calling PeerManager::timer_tick_occurred()");
                        peer_manager.timer_tick_occurred();
                    }
                    _ = cm_tick_timer.tick() => {
                        debug!("Calling ChannelManager::timer_tick_occurred()");
                        channel_manager.timer_tick_occurred();
                    }

                    // --- Persistence branches --- //
                    _ = ng_timer.tick() => {
                        debug!("Pruning and persisting network graph");
                        let network_graph = gossip_sync.network_graph();
                        network_graph.remove_stale_channels_and_tracking();
                        let persist_res = persister
                            .persist_graph(network_graph)
                            .await;
                        if let Err(e) = persist_res {
                            // The network graph isn't super important,
                            // but we still should log a warning.
                            warn!("Couldn't persist network graph: {:#}", e);
                        }
                    }
                    _ = ps_timer.tick() => {
                        debug!("Persisting probabilistic scorer");
                        let persist_res = persister
                            .persist_scorer(scorer.as_ref())
                            .await;
                        if let Err(e) = persist_res {
                            // The scorer isn't super important,
                            // but we still should log a warning.
                            warn!("Couldn't persist scorer: {e:#}");
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
            //   (e.g. `shutdown_after_sync_if_no_activity` is set), and since
            //   we're already another API call for the channel manager, we
            //   might as well concurrently persist these as well.
            let network_graph = gossip_sync.network_graph();
            let (cm_res, ng_res, ps_res) = tokio::join!(
                persister.persist_manager(channel_manager.deref()),
                persister.persist_graph(network_graph),
                persister.persist_scorer(scorer.as_ref()),
            );
            for res in [cm_res, ng_res, ps_res] {
                if let Err(e) = res {
                    error!("Final persistence failure: {e:#}");
                }
            }
        })
    }
}
