use std::{
    fmt::{self, Display},
    sync::Arc,
};

use anyhow::{anyhow, Context};
use common::{ln::channel::LxOutPoint, notify_once::NotifyOnce, task::LxTask};
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span};

use crate::{
    alias::LexeChainMonitorType, traits::LexePersister, BoxedAnyhowFuture,
};

/// Represents a channel monitor update. See docs on each field for details.
pub struct LxChannelMonitorUpdate {
    pub funding_txo: LxOutPoint,
    /// The ID of the channel monitor update, given by
    /// [`ChannelMonitorUpdate::update_id`] or
    /// [`ChannelMonitor::get_latest_update_id`].
    ///
    /// [`ChannelMonitorUpdate::update_id`]: lightning::chain::channelmonitor::ChannelMonitorUpdate::update_id
    /// [`ChannelMonitor::get_latest_update_id`]: lightning::chain::channelmonitor::ChannelMonitor::get_latest_update_id
    pub update_id: u64,
    /// A future which makes an api call (typically with retries) to the
    /// backend to persist the channel monitor state, returning an
    /// `anyhow::Result<()>` once either (1) persistence succeeds or (2) there
    /// were too many failures to keep trying. We take this future as input
    /// (instead of e.g. a `VfsFile`) because it is the cleanest and easiest
    /// way to abstract over the user node and LSP's differing api clients, vfs
    /// structures, and expected error types.
    pub api_call_fut: BoxedAnyhowFuture,
    pub kind: ChannelMonitorUpdateKind,
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
pub fn spawn_channel_monitor_persister_task<PS>(
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown: NotifyOnce,
) -> LxTask<()>
where
    PS: LexePersister,
{
    debug!("Starting channel monitor persister task");
    const SPAN_NAME: &str = "(chan-monitor-persister)";
    LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
        loop {
            tokio::select! {
                Some(update) = channel_monitor_persister_rx.recv() => {

                    let handle_result = handle_update(
                        chain_monitor.as_ref(),
                        update,
                    ).await;

                    if let Err(e) = handle_result {
                        error!("Monitor persist error: {e:#}");

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
    chain_monitor: &LexeChainMonitorType<PS>,
    update: LxChannelMonitorUpdate,
) -> anyhow::Result<()> {
    let LxChannelMonitorUpdate {
        funding_txo,
        update_id,
        api_call_fut,
        kind,
    } = update;

    debug!(%kind, %funding_txo, %update_id, "Handling channel monitor update");

    // Run the persist future.
    api_call_fut
        .await
        .with_context(|| format!("{kind} {funding_txo} {update_id}"))
        .context("Channel monitor persist API call failed")?;

    // Notify the chain monitor that the monitor update has been persisted.
    // - This should trigger a log like "Completed off-chain monitor update ..."
    // - NOTE: After this update, there may still be more updates to persist.
    //   The LDK log message will say "all off-chain updates complete" or "still
    //   have pending off-chain updates" (common during payments)
    // - NOTE: Only after *all* channel monitor updates are handled will the
    //   channel be reenabled and the BGP woken to process events via the chain
    //   monitor future.
    chain_monitor
        .channel_monitor_updated(OutPoint::from(funding_txo), update_id)
        .map_err(|e| anyhow!("{kind} {funding_txo} {update_id}: {e:?}"))
        .context("channel_monitor_updated returned Err")?;

    info!(%kind, %funding_txo, %update_id, "Success: persisted monitor");

    Ok(())
}
