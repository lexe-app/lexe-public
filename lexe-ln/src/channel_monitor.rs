use std::{
    fmt::{self, Display},
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context};
use common::{ln::channel::LxOutPoint, notify_once::NotifyOnce, task::LxTask};
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, Instrument};

use crate::{
    alias::LexeChainMonitorType,
    traits::{LexeChannelManager, LexeInnerPersister, LexePersister},
};

/// Represents a channel monitor update. See docs on each field for details.
pub struct LxChannelMonitorUpdate {
    #[allow(dead_code)] // Conceptually part of the update.
    kind: ChannelMonitorUpdateKind,
    funding_txo: LxOutPoint,
    /// The ID of the channel monitor update, given by
    /// [`ChannelMonitorUpdate::update_id`] or
    /// [`ChannelMonitor::get_latest_update_id`].
    ///
    /// [`ChannelMonitorUpdate::update_id`]: lightning::chain::channelmonitor::ChannelMonitorUpdate::update_id
    /// [`ChannelMonitor::get_latest_update_id`]: lightning::chain::channelmonitor::ChannelMonitor::get_latest_update_id
    update_id: u64,
    span: tracing::Span,
}

impl LxChannelMonitorUpdate {
    pub fn new(
        kind: ChannelMonitorUpdateKind,
        funding_txo: LxOutPoint,
        update_id: u64,
    ) -> Self {
        let span =
            info_span!("(monitor-update)", %kind, %funding_txo, %update_id);

        Self {
            kind,
            funding_txo,
            update_id,
            span,
        }
    }

    /// The span for this update which includes the full monitor update context.
    ///
    /// Logs related to this monitor update should be logged inside this span,
    /// to ensure the log information is associated with this update.
    pub fn span(&self) -> tracing::Span {
        self.span.clone()
    }
}

/// Whether the [`LxChannelMonitorUpdate`] represents a new or updated channel.
#[derive(Copy, Clone, Debug)]
pub enum ChannelMonitorUpdateKind {
    New,
    Updated,
}

impl Display for ChannelMonitorUpdateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => write!(f, "new"),
            Self::Updated => write!(f, "updated"),
        }
    }
}

/// Spawns a task which executes channel monitor persistence calls in serial.
/// This prevent a race conditions where two monitor updates come in quick
/// succession and the newer channel monitor state is overwritten by the older
/// channel monitor state.
///
/// # Shutdown
///
/// The shutdown sequence for this task is special. LDK has noted that it may be
/// possible to generate monitor updates to be persisted after disconnecting
/// from a peer. However, we also disconnect from all peers in our peer
/// connector task in response to a shutdown signal, meaning that if the monitor
/// persister task is scheduled first and shuts down immediately, it won't be
/// around anymore when those monitor updates are queued. Thus, we trigger the
/// shutdown for the monitor persister task only *after* the BGP has completed
/// its shutdown sequence (during which it repersists the channel manager).
///
/// <https://discord.com/channels/915026692102316113/1367736643100086374/1367952226269663262>
pub fn spawn_channel_monitor_persister_task<CM, PS>(
    persister: PS,
    channel_manager: CM,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut monitor_persister_shutdown: NotifyOnce,
    shutdown: NotifyOnce,
) -> LxTask<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    debug!("Starting channel monitor persister task");
    const SPAN_NAME: &str = "(chan-monitor-persister)";
    LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
        loop {
            tokio::select! {
                Some(update) = channel_monitor_persister_rx.recv() => {
                    let update_span = update.span();

                    let handle_result = handle_update(
                        &persister,
                        chain_monitor.as_ref(),
                        update,
                    )
                        .instrument(update_span.clone())
                        .await;

                    if let Err(e) = handle_result {
                        update_span.in_scope(|| {
                            error!("Fatal: Monitor persist error: {e:#}");
                        });

                        // Channel monitor persistence errors are fatal.
                        // Return immediately to prevent further monitor
                        // persists (which may skip the current monitor update
                        // if using incremental persist)
                        shutdown.send();
                        return;
                    }
                }
                () = monitor_persister_shutdown.recv() => {
                    debug!("channel monitor persister task beginning shutdown");
                    break;
                }
            }
        }

        // After we've received a shutdown signal, ensure both the channel
        // manager and channel monitors have reached a quiescent state.
        // Wait for channel monitor updates or channel manager persists until
        // neither do anything for a full 10ms. The 10ms delay allows other
        // tasks which may trigger these persists to be scheduled.
        const QUIESCENT_TIMEOUT: Duration = Duration::from_millis(10);
        loop {
            tokio::select! {
                biased;
                () = channel_manager
                    .get_event_or_persistence_needed_future() => {
                    if channel_manager.get_and_clear_needs_persistence() {
                        let try_persist = persister
                            .persist_manager(channel_manager.deref())
                            .await;
                        if let Err(e) = try_persist {
                            error!("(Quiescence) manager persist error: {e:#}");
                            // Nothing to do if persist fails, so just shutdown.
                            return;
                        }
                    }
                }
                Some(update) = channel_monitor_persister_rx.recv() => {
                    let update_span = update.span();

                    let handle_result = handle_update(
                        &persister,
                        chain_monitor.as_ref(),
                        update,
                    )
                        .instrument(update_span.clone())
                        .await;

                    if let Err(e) = handle_result {
                        update_span.in_scope(|| {
                            error!("(Quiescence) Monitor persist error: {e:#}");
                        });
                        // Nothing to do if persist fails, so just shutdown.
                        return;
                    }
                }
                _ = tokio::time::sleep(QUIESCENT_TIMEOUT) => {
                    info!("Channel mgr and monitors quiescent; shutting down.");
                    return;
                }
            };
        }
    })
}

/// A helper to prevent [`spawn_channel_monitor_persister_task`]'s control flow
/// from getting too complex.
///
/// Since channel monitor persistence is very important, all [`Err`]s are
/// considered fatal; the caller should send a shutdown signal and exit.
async fn handle_update<PS: LexePersister>(
    persister: &PS,
    chain_monitor: &LexeChainMonitorType<PS>,
    update: LxChannelMonitorUpdate,
) -> anyhow::Result<()> {
    debug!("Handling channel monitor update");

    // Persist the monitor.
    let funding_txo = OutPoint::from(update.funding_txo);
    persister
        .persist_channel_monitor(chain_monitor, &update.funding_txo)
        .await
        .context("persist_monitor failed")?;

    // Notify the chain monitor that the monitor update has been persisted.
    // - This should trigger a log like "Completed off-chain monitor update ..."
    // - NOTE: After this update, there may still be more updates to persist.
    //   The LDK log message will say "all off-chain updates complete" or "still
    //   have pending off-chain updates" (common during payments)
    // - NOTE: Only after *all* channel monitor updates are handled will the
    //   channel be reenabled and the BGP woken to process events via the chain
    //   monitor future.
    chain_monitor
        .channel_monitor_updated(funding_txo, update.update_id)
        .map_err(|e| anyhow!("channel_monitor_updated returned Err: {e:?}"))?;

    info!("Success: persisted monitor");

    Ok(())
}
