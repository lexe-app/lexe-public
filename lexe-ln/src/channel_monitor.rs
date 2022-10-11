use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use common::ln::channel::LxOutPoint;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::chainmonitor::MonitorUpdateId;
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::alias::LexeChainMonitorType;
use crate::traits::LexePersister;

/// The maximum number of channel monitor persist requests that can be sent to
/// the channel monitor persistence task at once. This needs to be a bit higher
/// than the default channel size because a bunch of channel monitor persist
/// calls might be generated all at once by chain sync during init.
// 2016 is the expected number of blocks generated over two weeks.
pub const CHANNEL_MONITOR_PERSIST_TASK_CHANNEL_SIZE: usize = 2016;

type BoxedAnyhowFuture =
    Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Represents a channel monitor update. See docs on each field for details.
pub struct LxChannelMonitorUpdate {
    pub funding_txo: LxOutPoint,
    pub update_id: MonitorUpdateId,
    /// A [`Future`] which makes an api call (typically with with retries) to
    /// the backend to persist the channel monitor state, returning an
    /// `anyhow::Result<()>` once either persistence succeeds or there were too
    /// many failures to keep trying. We take this future as input (instead of
    /// e.g. a `NodeFile`) because it is the cleanest and easiest way to
    /// abstract over the user node and LSP's differing api clients, vfs
    /// structures, and expected error types.
    ///
    /// NOTE: The future passed in should persist the *total state* of the
    /// channel monitor, rather than the incremental updates represented by
    /// [`ChannelMonitorUpdate`].
    ///
    /// NOTE: Because the channel monitor persister task amortizes persist
    /// calls, implementing code should expect that only the latest [`Future`]
    /// in a series of updates made in quick succession will actually be
    /// executed by the channel monitor persister task.
    ///
    /// [`ChannelMonitorUpdate`]: lightning::chain::channelmonitor::ChannelMonitorUpdate
    pub api_call_fut: BoxedAnyhowFuture,
    /// The sequence number of the channel monitor update, given by
    /// [`ChannelMonitorUpdate::update_id`]. Is [`None`] for new channels and
    /// updates triggered by chain sync.
    ///
    /// [`ChannelMonitorUpdate`]: lightning::chain::channelmonitor::ChannelMonitorUpdate
    /// [`ChannelMonitorUpdate::update_id`]: lightning::chain::channelmonitor::ChannelMonitorUpdate::update_id
    pub sequence_num: Option<u64>,
    pub kind: ChannelMonitorUpdateKind,
}

/// Whether the [`LxChannelMonitorUpdate`] represents a new or updated channel.
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
/// This prevents race conditions where chain sync can generate hundreds of
/// channel monitor persist calls in the span of just a few hundred ms,
/// overwhelming the backend and exhausting all local file descriptors.
///
/// In addition, this task amortizes over a series of persistence updates made
/// in quick succession by only executing the *last* future in the series. For
/// example, if 100 updates are made all at once, the task only runs the 100th
/// future. If the 100th persist future succeeds, the [`MonitorUpdateId`] of all
/// 100 updates is passed into [`ChainMonitor::channel_monitor_updated`] and
/// channel operation can continue.
///
/// TODO(max): Actually implement the amortization of persist calls.
///
/// [`ChainMonitor::channel_monitor_updated`]: lightning::chain::chainmonitor::ChainMonitor
pub fn spawn_channel_monitor_persister_task<PS>(
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    PS: LexePersister,
{
    debug!("Starting channel_monitor_updated task");
    LxTask::spawn_named("channel monitor persister", async move {
        loop {
            tokio::select! {
                Some(update) = channel_monitor_persister_rx.recv() => {
                    let seq = update.sequence_num;
                    let kind = update.kind;
                    debug!("Running {kind} channel persist future (#{seq:?})");

                    let persist_result = tokio::select! {
                        res = update.api_call_fut => res,
                        () = shutdown.recv() => break,
                    };

                    match persist_result {
                        Ok(()) => {
                            debug!(
                                "Success: persisted {kind} channel (#{seq:?})",
                            );

                            // Update the chain monitor
                            if let Err(e) = chain_monitor
                                .channel_monitor_updated(
                                    OutPoint::from(update.funding_txo),
                                    update.update_id,
                                ) {
                                error!(
                                    "Chain monitor returned err: {e:?} (#{seq:?})"
                                );

                                // If the update wasn't accepted, the channel is
                                // disabled, so no transactions can be made.
                                // Just shut down.
                                shutdown.send();
                            }
                        }
                        Err(e) => {
                            // Channel monitor persistence errors are serious;
                            // shut down to prevent any loss of funds.
                            error!(
                                "Couldn't persist {kind} channel: {e:#} (#{seq:?})",
                            );
                            shutdown.send();
                        }
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
