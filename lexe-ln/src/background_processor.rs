use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::util::events::EventsProvider;
use tokio::time::{interval, interval_at, Instant};
use tracing::{debug, error, info, instrument, trace, warn};

use crate::alias::{
    LexeChainMonitorType, P2PGossipSyncType, ProbabilisticScorerType,
};
use crate::traits::{
    LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
};

const PROCESS_EVENTS_INTERVAL: Duration = Duration::from_millis(1000);
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
                tokio::select! {
                    // --- Event branches --- //
                    _ = process_timer.tick() => {
                        trace!("Processing pending events");
                        channel_manager
                            .process_pending_events(&event_handler);
                        chain_monitor
                            .process_pending_events(&event_handler);
                        peer_manager.process_events();
                    }
                    _ = pm_timer.tick() => {
                        debug!("Calling PeerManager::timer_tick_occurred()");
                        peer_manager.timer_tick_occurred();
                    }
                    _ = cm_tick_timer.tick() => {
                        debug!("Calling ChannelManager::timer_tick_occurred()");
                        channel_manager.timer_tick_occurred();
                    }

                    // --- Persistence branches --- //
                    _ = channel_manager.get_persistable_update_future() => {
                        debug!("Persisting channel manager");
                        let persist_res = persister
                            .persist_manager(channel_manager.deref())
                            .await;
                        if let Err(e) = persist_res {
                            // Failing to persist the channel manager won't
                            // lose funds so long as the chain monitors have
                            // been persisted correctly, but it's still
                            // serious - initiate a shutdown
                            error!("Couldn't persist channel manager: {:#}", e);
                            shutdown.send();
                            break;
                        }
                    }
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
