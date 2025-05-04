use std::{sync::Arc, time::Duration};

use common::{notify_once::NotifyOnce, task::LxTask, time::DisplayMs};
use tokio::time::Instant;
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::{
    alias::LexeChainMonitorType,
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
    /// Peer manager ticks.
    pub const PEER_MANAGER: Duration = Duration::from_secs(15);
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
    pub const PEER_MANAGER: Duration = Duration::from_millis(400);
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
    monitor_persister_shutdown: NotifyOnce,
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
            let bgp_start = Instant::now();

            let mk_interval = |delay: Duration, interval: Duration| {
                // Remove the staggering in debug mode in an attempt to catch
                // any subtle race conditions which may arise
                let timer_start = if cfg!(debug_assertions) {
                    bgp_start
                } else {
                    bgp_start + delay
                };
                tokio::time::interval_at(timer_start, interval)
            };

            let mut process_events_timer =
                mk_interval(delay::PROCESS_EVENTS, interval::PROCESS_EVENTS);
            let mut pm_timer =
                mk_interval(delay::PEER_MANAGER, interval::PEER_MANAGER);
            let mut cm_timer =
                mk_interval(delay::CHANNEL_MANAGER, interval::CHANNEL_MANAGER);

            // This is the event handler future generator type required by LDK
            let mk_event_handler_fut =
                |event| event_handler.get_ldk_handler_future(event);

            loop {
                // A future that completes when any of the following applies:
                //
                // - Our process events timer ticked
                // - The channel manager got an update (event or repersist)
                // - The chain monitor got an update (typically that all updates
                //   were persisted for a channel monitor)
                // - The onion messenger got an update
                let process_events_fut = async {
                    tokio::select! {
                        biased;
                        _ = process_events_timer.tick() =>
                            debug!("Triggered: process_events_timer ticked"),
                        () = channel_manager
                            .get_event_or_persistence_needed_future() =>
                            debug!("Triggered: Channel manager update"),
                        () = chain_monitor.get_update_future() =>
                            debug!("Triggered: Chain monitor update"),
                    };

                    // We're about to process events. Prevent duplicate work by
                    // resetting the process_events_timer & clearing out the
                    // process_events channel.
                    process_events_timer.reset();
                };

                tokio::select! {
                    () = process_events_fut => {
                        debug!("Processing pending events");
                        let process_start = Instant::now();

                        channel_manager
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(events)(chan-man)"))
                            .await;
                        chain_monitor
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(events)(chain-mon)"))
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
                        }.instrument(info_span!("(events)(peer-man)")).await;

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

                        let elapsed = process_start.elapsed();
                        let elapsed_ms = DisplayMs(elapsed);
                        if elapsed > Duration::from_secs(10) {
                            warn!("Event processing took {elapsed_ms}");
                        } else if elapsed > Duration::from_secs(1) {
                            info!("Event processing took {elapsed_ms}");
                        } else {
                            debug!("Event processing took {elapsed_ms}");
                        }
                    }

                    _ = pm_timer.tick() =>
                        peer_manager.timer_tick_occurred(),

                    _ = cm_timer.tick() =>
                        channel_manager.timer_tick_occurred(),

                    () = shutdown.recv() =>
                        break debug!("Background processor shutting down"),
                }
            }

            // Persist the manager one last time. This may prevent some races
            // where the node quits while channel updates were in-flight,
            // causing ChannelMonitor updates to be persisted without
            // corresponding ChannelManager updates being persisted.
            // This does not risk the loss of funds, but upon next boot the
            // ChannelManager may accidentally trigger a force close.
            channel_manager.get_and_clear_needs_persistence();
            if let Err(e) = persister.persist_manager(&*channel_manager).await {
                error!("Final channel manager persistence failure: {e:#}");
            }

            // The monitor persister task should only begin shutdown once the
            // BGP has shut down, in case this final channel manager persist (or
            // peer disconnect at shutdown) triggers more monitor updates.
            monitor_persister_shutdown.send();
        },
    )
}
