use std::{ops::Range, pin::Pin, sync::Arc, time::Duration};

use anyhow::{Context as _, anyhow};
use futures::FutureExt;
use lexe_common::time::DisplayMs;
use lexe_crypto::rng::{RngExt, ThreadFastRng};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::ln::msgs::RoutingMessageHandler;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tracing::{Instrument, debug, error, info, info_span, trace, warn};

use crate::{
    alias::LexeChainMonitorType,
    channel_monitor::{self, ChannelMonitorPersisterCommand},
    persister::LexePersisterMethods,
    traits::{
        LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
    },
};

/// The intervals for the timers used in the BGP.
mod interval {
    use std::time::Duration;

    /// ChainMonitor::archive_fully_resolved_channel_monitors ticks.
    pub const ARCHIVE: Duration = Duration::from_secs(10 * 60);
    /// Channel manager ticks.
    pub const CHANNEL_MANAGER: Duration = Duration::from_secs(60);
    /// ChainMonitor::rebroadcast_pending_claims ticks.
    pub const REBROADCAST: Duration = Duration::from_secs(30);
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

    pub const ARCHIVE: Duration = Duration::from_secs(15);
    pub const CHANNEL_MANAGER: Duration = Duration::from_secs(60);
    pub const REBROADCAST: Duration = Duration::from_secs(30);
    pub const PEER_MANAGER: Duration = Duration::from_millis(400);
    pub const PROCESS_EVENTS: Duration = Duration::from_millis(800);
}

/// An event sent over the `htlcs_forwarded_bus` that indicates a call to
/// `process_pending_htlc_forwards` was complete.
#[derive(Copy, Clone, Debug)]
pub struct HtlcsForwarded;

/// A handle for test-only background processor controls.
#[derive(Clone)]
pub struct BackgroundProcessorHandle {
    quiescence_tx: mpsc::Sender<QuiescenceRequest>,
}

/// A test-only request to wait for BGP quiescence.
struct QuiescenceRequest {
    response: oneshot::Sender<anyhow::Result<()>>,
}

/// A fatal error that occurs in the BGP.
struct FatalError;

impl BackgroundProcessorHandle {
    /// Test-only: waits for LDK event and HTLC quiescence.
    pub async fn wait_quiescent(&self) -> anyhow::Result<()> {
        let (response, receiver) = oneshot::channel();
        self.quiescence_tx
            .send(QuiescenceRequest { response })
            .await
            .map_err(|_| anyhow!("Background processor stopped"))?;
        receiver
            .await
            .context("Background processor canceled quiescence request")?
    }
}

/// A Tokio-native background processor that runs on a single task and does not
/// spawn any OS threads. Modeled after the lightning-background-processor crate
/// provided by LDK - see that crate's implementation for more details.
pub fn start<CM, PM, PS, EH, RMH>(
    channel_manager: CM,
    peer_manager: PM,
    persister: PS,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    channel_monitor_persister_tx: mpsc::Sender<ChannelMonitorPersisterCommand>,
    event_handler: EH,
    // The range (in millis) from which to pick a random forwarding delay.
    forward_delay_range_ms: Range<u32>,
    htlcs_forwarded_bus: EventsBus<HtlcsForwarded>,
    monitor_persister_shutdown: NotifyOnce,
    mut shutdown: NotifyOnce,
) -> (BackgroundProcessorHandle, LxTask<()>)
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS, RMH>,
    PS: LexePersister,
    EH: LexeEventHandler,
    RMH: RoutingMessageHandler,
{
    let (quiescence_tx, mut quiescence_rx) = mpsc::channel(1);
    let handle = BackgroundProcessorHandle { quiescence_tx };

    let task = LxTask::spawn_with_span(
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
            let mut rebroadcast_timer =
                mk_interval(delay::REBROADCAST, interval::REBROADCAST);
            let mut archive_timer =
                mk_interval(delay::ARCHIVE, interval::ARCHIVE);

            // Optional future for the HTLC forwarding delay. Set to Some when
            // we first detect pending HTLCs and None after processing them.
            let mut forward_delay_timer = None::<Pin<Box<tokio::time::Sleep>>>;

            // Tick CM once at startup.
            channel_manager.timer_tick_occurred();
            // Rebroadcast pending claims at startup.
            chain_monitor.rebroadcast_pending_claims();

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
                        let result = process_events(
                            &channel_manager,
                            &chain_monitor,
                            &event_handler,
                            &forward_delay_range_ms,
                            &mut forward_delay_timer,
                            &peer_manager,
                            &persister,
                            &mut rng,
                            &shutdown,
                        ).await;
                        if let Err(FatalError) = result {
                            break;
                        }
                    }

                    Some(request) = quiescence_rx.recv() => {
                        // Handle a test-only quiescence request.
                        process_events_timer.reset();
                        let result = process_until_quiescent(
                            &channel_manager,
                            &chain_monitor,
                            &channel_monitor_persister_tx,
                            &event_handler,
                            &forward_delay_range_ms,
                            &mut forward_delay_timer,
                            &htlcs_forwarded_bus,
                            &peer_manager,
                            &persister,
                            &mut rng,
                            &mut shutdown,
                        ).await;

                        let should_continue = result.is_ok();
                        let result = result.map_err(|FatalError| {
                            anyhow!("BGP fatal error waiting for quiescence")
                        });
                        let _ = request.response.send(result);
                        if !should_continue {
                            break;
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
                        process_pending_htlc_forwards(
                            &channel_manager,
                            &mut forward_delay_timer,
                            &htlcs_forwarded_bus,
                        );
                    }

                    _ = pm_timer.tick() =>
                        peer_manager.timer_tick_occurred(),

                    _ = cm_timer.tick() =>
                        channel_manager.timer_tick_occurred(),

                    _ = rebroadcast_timer.tick() =>
                        chain_monitor.rebroadcast_pending_claims(),

                    _ = archive_timer.tick() =>
                        chain_monitor.archive_fully_resolved_channel_monitors(),

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
    );

    (handle, task)
}

/// Processes one pass of pending LDK events and persists manager changes.
async fn process_events<CM, PM, PS, EH, RMH>(
    channel_manager: &CM,
    chain_monitor: &LexeChainMonitorType<PS>,
    event_handler: &EH,
    forward_delay_range_ms: &Range<u32>,
    forward_delay_timer: &mut Option<Pin<Box<tokio::time::Sleep>>>,
    peer_manager: &PM,
    persister: &PS,
    rng: &mut ThreadFastRng,
    shutdown: &NotifyOnce,
) -> Result<(), FatalError>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS, RMH>,
    PS: LexePersister,
    EH: LexeEventHandler,
    RMH: RoutingMessageHandler,
{
    trace!("Processing pending events");
    let process_start = Instant::now();

    // This is the event handler future generator type required by LDK.
    let mk_event_handler_fut =
        |event| event_handler.get_ldk_handler_future(event);

    // NOTE: Event processing + channel manager persist matches LDK's BGP
    // implementation ordering. LDK notes that `PeerManager::process_events`
    // may block on ChannelManager's locks, hence it comes after async event
    // handling. When the ChannelManager finishes whatever it's doing, we want
    // to start persisting it as quickly as possible.
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
        // NOTE(phlip9): worried the `Connection` -> `process_events` flow might
        // starve the BGP if it grabs the `process_events` lock and is forced to
        // do a neverending amount of work under load.
        //
        // TODO(phlip9): Consider sending a notification to the new
        // `process_events` task and waiting for that to complete?
        peer_manager.process_events();
    }
    .instrument(info_span!("(events)(peer-man)"))
    .await;

    // If any HTLCs need forwarding, the channel manager's
    // `.get_event_or_persistence_needed_future()` will be notified, bringing
    // us here. Start a randomized forwarding delay to batch nearby HTLCs and
    // make timing analysis harder.
    // https://delvingbitcoin.org/t/latency-and-privacy-in-lightning/1723#p-5107-understanding-forwarding-delays-privacy-1
    if forward_delay_timer.is_none()
        && channel_manager.needs_pending_htlc_processing()
    {
        let delay_ms = rng.gen_range_u32(forward_delay_range_ms.clone());
        let delay = Duration::from_millis(u64::from(delay_ms));
        let sleep_fut = tokio::time::sleep(delay);
        *forward_delay_timer = Some(Box::pin(sleep_fut));
        trace!("Started HTLC forward timer: {delay_ms}ms");
    }

    if channel_manager.get_and_clear_needs_persistence() {
        let try_persist = persister.persist_manager(&**channel_manager).await;
        if let Err(e) = try_persist {
            // Failing to persist the channel manager won't lose funds so long
            // as the chain monitors have been persisted correctly, but it's
            // still serious - initiate a shutdown.
            error!("Channel manager persist error: {e:#}");
            shutdown.send();
            return Err(FatalError);
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

    Ok(())
}

/// Processes delayed HTLC forwards and clears their timer.
fn process_pending_htlc_forwards<CM, PS>(
    channel_manager: &CM,
    forward_delay_timer: &mut Option<Pin<Box<tokio::time::Sleep>>>,
    htlcs_forwarded_bus: &EventsBus<HtlcsForwarded>,
) where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    debug!("Processing pending HTLC forwards");
    channel_manager.process_pending_htlc_forwards();

    htlcs_forwarded_bus.send(HtlcsForwarded);
    *forward_delay_timer = None;
}

/// Test-only: processes LDK events until all event sources and HTLCs are
/// quiescent.
async fn process_until_quiescent<CM, PM, PS, EH, RMH>(
    channel_manager: &CM,
    chain_monitor: &LexeChainMonitorType<PS>,
    channel_monitor_persister_tx: &mpsc::Sender<ChannelMonitorPersisterCommand>,
    event_handler: &EH,
    forward_delay_range_ms: &Range<u32>,
    forward_delay_timer: &mut Option<Pin<Box<tokio::time::Sleep>>>,
    htlcs_forwarded_bus: &EventsBus<HtlcsForwarded>,
    peer_manager: &PM,
    persister: &PS,
    rng: &mut ThreadFastRng,
    shutdown: &mut NotifyOnce,
) -> Result<(), FatalError>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS, RMH>,
    PS: LexePersister,
    EH: LexeEventHandler,
    RMH: RoutingMessageHandler,
{
    loop {
        process_events(
            channel_manager,
            chain_monitor,
            event_handler,
            forward_delay_range_ms,
            forward_delay_timer,
            peer_manager,
            persister,
            rng,
            shutdown,
        )
        .await?;

        // Snapshot HTLC state before flushing so any monitor update which made
        // that state visible is included in the flush boundary.
        let has_pending_htlcs =
            channel_manager_has_pending_htlcs(channel_manager);
        let has_pending_forwards = forward_delay_timer.is_some();
        channel_monitor::wait_flush(channel_monitor_persister_tx)
            .await
            .map_err(|err| {
                error!("Failed to flush chanmon persist: {err:#}");
                FatalError
            })?;
        if !ldk_event_sources_quiescent(channel_manager, chain_monitor) {
            continue;
        }

        if !has_pending_htlcs && !has_pending_forwards {
            return Ok(());
        }

        // A terminal payment event may precede the final commitment dance.
        // Wait for peer activity or delayed forwarding to advance the HTLCs.
        debug!("Waiting for pending HTLC work");
        tokio::select! {
            () = channel_manager
                .get_event_or_persistence_needed_future() => (),
            () = chain_monitor.get_update_future() => (),
            _ = async {
                Pin::new(&mut *forward_delay_timer)
                    .as_pin_mut()
                    .unwrap()
                    .await
            }, if forward_delay_timer.is_some() => {
                process_pending_htlc_forwards(
                    channel_manager,
                    forward_delay_timer,
                    htlcs_forwarded_bus,
                );
            }
            () = shutdown.recv() => {
                error!("BGP shutdown while waiting for quiescence");
                return Err(FatalError);
            }
        }
    }
}

/// Test-only: returns whether any channel has unresolved HTLCs.
fn channel_manager_has_pending_htlcs<CM, PS>(channel_manager: &CM) -> bool
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // TODO(phlip9): Add `ChannelManager::has_pending_htlcs` to our LDK fork
    // to avoid building channel details for this check.
    channel_manager.list_channels().iter().any(|channel| {
        !channel.pending_inbound_htlcs.is_empty()
            || !channel.pending_outbound_htlcs.is_empty()
    })
}

/// Test-only: returns whether neither LDK event source has work ready.
fn ldk_event_sources_quiescent<CM, PS>(
    channel_manager: &CM,
    chain_monitor: &LexeChainMonitorType<PS>,
) -> bool
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    if channel_manager
        .get_event_or_persistence_needed_future()
        .now_or_never()
        .is_some()
    {
        return false;
    }

    chain_monitor.get_update_future().now_or_never().is_none()
}
