use std::{ops::Deref, pin::Pin, sync::Arc, time::Duration};

use common::{
    rng::{Rng, ThreadFastRng},
    time::DisplayMs,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::ln::msgs::RoutingMessageHandler;
use rand::distributions::uniform::SampleRange;
use tokio::time::Instant;
use tracing::{Instrument, debug, error, info, info_span, trace, warn};

use crate::{
    alias::LexeChainMonitorType,
    event::HtlcsForwarded,
    persister::LexePersisterMethods,
    traits::{
        LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
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
pub fn start<CM, PM, PS, EH, RMH>(
    channel_manager: CM,
    peer_manager: PM,
    persister: PS,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    event_handler: EH,
    // The range (in millis) from which to pick a random forwarding delay.
    forward_delay_range_ms: impl SampleRange<u64> + Clone + Send + Sync + 'static,
    htlcs_forwarded_bus: EventsBus<HtlcsForwarded>,
    monitor_persister_shutdown: NotifyOnce,
    mut shutdown: NotifyOnce,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS, RMH>,
    PS: LexePersister,
    EH: LexeEventHandler,
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
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

            let mut rng = ThreadFastRng::new();

            let mut process_events_timer =
                mk_interval(delay::PROCESS_EVENTS, interval::PROCESS_EVENTS);
            let mut pm_timer =
                mk_interval(delay::PEER_MANAGER, interval::PEER_MANAGER);
            let mut cm_timer =
                mk_interval(delay::CHANNEL_MANAGER, interval::CHANNEL_MANAGER);

            // This is the event handler future generator type required by LDK
            let mk_event_handler_fut =
                |event| event_handler.get_ldk_handler_future(event);

            // Optional future for the HTLC forwarding delay. Set to Some when
            // we first detect pending HTLCs and None after processing them.
            let mut forward_delay_timer = None::<Pin<Box<tokio::time::Sleep>>>;

            loop {
                // A future that completes when any of the following applies:
                //
                // - Our process events timer ticked
                // - The channel manager got a new event, needs persistence, or
                //   there are pending HTLCs to be forwarded.
                // - The chain monitor got an update (typically that all updates
                //   were persisted for a channel monitor)
                let process_events_fut = async {
                    tokio::select! {
                        biased;
                        _ = process_events_timer.tick() =>
                            trace!("Triggered: process_events_timer ticked"),
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
                        trace!("Processing pending events");
                        let process_start = Instant::now();

                        // NOTE: Event processing + channel manager persist
                        // matches LDK's BGP implementation ordering.
                        // LDK notes that "`PeerManager::process_events` may
                        // block on ChannelManager's locks, hence it comes
                        // [after async event handling]. When the ChannelManager
                        // finishes whatever it's doing, we want to ensure we
                        // start persisting the channel manager as quickly as we
                        // can, especially without [async event processing]."

                        channel_manager
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(events)(chan-man)"))
                            .await;
                        chain_monitor
                            .process_pending_events_async(mk_event_handler_fut)
                            .instrument(info_span!("(events)(chain-mon)"))
                            .await;
                        // NOTE: Onion messenger events are handled by the
                        // OnionMessengerEventHandler.

                        // Wrapped in a future for instrumentation only.
                        async {
                            // NOTE(phlip9): worried the `Connection` ->
                            // `process_events` flow might starve the BGP if it
                            // grabs the `process_events` lock and is forced to
                            // do a neverending amount of work under load.
                            //
                            // TODO(phlip9): Consider sending a notification to
                            // the new `process_events` task and waiting for
                            // that to complete?
                            peer_manager.process_events();
                        }.instrument(info_span!("(events)(peer-man)")).await;

                        // If any HTLCs need forwarding, the channel manager's
                        // `.get_event_or_persistence_needed_future()` will be
                        // notified, bringing us here. Here, we start a timer
                        // with a random delay to forward the HTLCs, if not
                        // already started. This randomized forwarding delay:
                        // (1) batches HTLCs that arrive close together and
                        // (2) makes timing analysis harder, improving privacy.
                        // https://delvingbitcoin.org/t/latency-and-privacy-in-lightning/1723#p-5107-understanding-forwarding-delays-privacy-1
                        //
                        // TODO(max): Currently, the HTLC forwarding timer below
                        // is disabled, as the LSP's `HTLCIntercepted` handler
                        // currently relies on a `PendingHTLCsForwardable` event
                        // to wait on HTLC forwarding via `htlcs_forwarded_bus`.
                        // We can't just disable the `PendingHTLCsForwardable`
                        // handling and replace it with the logic below, as
                        // channel_manager.needs_pending_htlc_processing() is
                        // only available in LDK v0.2, meaning that we'd have to
                        // fall back to polling for HTLC forwards, which would
                        // cause spurious wakeups of the HTLCIntercepted handler
                        // before any HTLCs were actually forwarded. An
                        // alternative approach is to have the HTLCIntercepted
                        // poll for a change in the channel balance, but that's
                        // hacky and not worth pursuing especially as we'll
                        // switch back to the original behavior (waiting on
                        // `htlcs_forwarded_bus`) once LDK v0.2 is released.
                        // Thus, we leave in the PendingHTLCsForwardable-based
                        // forwarding logic, and once we upgrade to LDK v0.2
                        // which removes the PendingHTLCsForwardable event, we
                        // can just delete all PendingHTLCsForwardable handling
                        // logic and uncomment the if statement below.
                        if false
                        // if forward_delay_timer.is_none()
                        //     && channel_manager.needs_pending_htlc_processing()
                        {
                            let delay_ms =
                                rng.gen_range(forward_delay_range_ms.clone());
                            let delay = Duration::from_millis(delay_ms);
                            let sleep_fut = tokio::time::sleep(delay);
                            forward_delay_timer = Some(Box::pin(sleep_fut));
                            trace!("Started HTLC forward timer: {delay_ms}ms");
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

                    // If the HTLC forward timer elapses,
                    // process pending HTLC forwards and clear the timer.
                    //
                    // About this weird Option<impl Future> polling:
                    // https://github.com/tokio-rs/tokio/issues/2583#issuecomment-638212772
                    _ = async {
                        Pin::new(&mut forward_delay_timer)
                            .as_pin_mut()
                            .unwrap()
                            .await
                    }, if forward_delay_timer.is_some() => {
                        debug!("Processing pending HTLC forwards");
                        channel_manager.process_pending_htlc_forwards();

                        htlcs_forwarded_bus.send(HtlcsForwarded);
                        forward_delay_timer = None;
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
