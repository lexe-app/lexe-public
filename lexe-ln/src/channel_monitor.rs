use std::{
    fmt::{self, Display},
    sync::Arc,
};

use anyhow::{anyhow, Context};
use common::{ln::channel::LxOutPoint, notify_once::NotifyOnce, task::LxTask};
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, Instrument};

use crate::{
    alias::LexeChainMonitorType,
    traits::{LexeInnerPersister, LexePersister},
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
pub fn spawn_channel_monitor_persister_task<PS: LexePersister>(
    persister: PS,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
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
                            error!("Monitor persist error: {e:#}");
                        });

                        // Channel monitor persistence errors are serious;
                        // all errors are considered fatal.
                        // Shut down to prevent any loss of funds.
                        shutdown.send();
                        break;
                    }
                }
                () = shutdown.recv() => {
                    info!("channel monitor persister task shutting down");
                    break;
                }
            }
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
        .persist_monitor(chain_monitor, &update.funding_txo)
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
