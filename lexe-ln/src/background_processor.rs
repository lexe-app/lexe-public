use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::notify;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::util::events::EventsProvider;
use tokio::time::{interval, interval_at, Instant};
use tracing::{debug, error, info, instrument, warn};

use crate::alias::{
    LexeChainMonitorType, P2PGossipSyncType, ProbabilisticScorerType,
};
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{
    LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
};

// It would be nice to get rid of the `PROCESS_EVENTS_INTERVAL` entirely and
// replace it with an LDK-provided future which resolves immediately after an
// event is made available for processing, but it isn't supported yet:
// https://github.com/lightningdevkit/rust-lightning/issues/2052
// So long as LDK's background processor still has a 100ms timer, indicating
// that the future proposed in #2052 isn't guaranteed to resolve when events are
// available, we should keep `PROCESS_EVENTS_INTERVAL` around as a backup.
const PROCESS_EVENTS_INTERVAL: Duration = Duration::from_secs(60);
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
    #[instrument(skip_all, name = "[background processor]")]
    #[allow(clippy::too_many_arguments)]
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
        // get_persistable_update_future() to resolve. Current known
        // cases include:
        //
        // 1) The successful completion of a channel monitor persist (which may
        //    resume monitor updating / broadcast a funding transaction)
        // 2) Opening a channel (which generates a channel open event)
        // 3) Sending a payment (which generates a payment sent event)
        //
        // We may be able to get rid of this once LDK#2052 is implemented. See
        // the comment above `PROCESS_EVENTS_INTERVAL` for more info.
        mut process_events_rx: notify::Receiver,
        test_event_tx: TestEventSender,
        mut shutdown: ShutdownChannel,
    ) -> LxTask<()>
    where
        CM: LexeChannelManager<PS>,
        PM: LexePeerManager<CM, PS>,
        PS: LexePersister,
        EH: LexeEventHandler,
    {
        LxTask::spawn_named("background processor", async move {
            let mut process_timer = interval(PROCESS_EVENTS_INTERVAL);
            let mut pm_timer = interval(PEER_MANAGER_PING_INTERVAL);
            let mut cm_tick_timer = interval(CHANNEL_MANAGER_TICK_INTERVAL);
            let start = Instant::now() + NETWORK_GRAPH_INITIAL_DELAY;
            let mut ng_timer = interval_at(start, NETWORK_GRAPH_PRUNE_INTERVAL);
            let mut ps_timer = interval(PROB_SCORER_PERSIST_INTERVAL);

            loop {
                // A future that completes whenever we need to reprocess events.
                // Returns a bool indicating whether we also need to repersist
                // the channel manager. NOTE: The "channel manager update"
                // branch is intentionally included here because LDK's
                // background processor always processes events just prior to
                // any channel manager persist.
                let process_events_fut = async {
                    tokio::select! {
                        biased;
                        () = channel_manager.get_persistable_update_future() => {
                            debug!("Channel manager got persistable update");
                            true
                        }
                        _ = process_timer.tick() => {
                            debug!("process_timer ticked");
                            false
                        }
                        () = process_events_rx.recv() => {
                            debug!("Triggered by process_events channel");
                            false
                        }
                    }
                };

                tokio::select! {
                    // --- Process events + channel manager repersist --- //
                    repersist_channel_manager = process_events_fut => {
                        debug!("Processing pending events");
                        channel_manager
                            .process_pending_events(&event_handler);
                        chain_monitor
                            .process_pending_events(&event_handler);
                        peer_manager.process_events();
                        test_event_tx.send(TestEvent::EventsProcessed);

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
                            warn!("Couldn't persist network graph: {:#}", e);
                        }
                    }

                    // --- Shutdown branch --- //
                    () = shutdown.recv() => {
                        info!("Background processor shutting down");
                        break;
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
                    error!("Final persistence failure: {:#}", e);
                }
            }
        })
    }
}
