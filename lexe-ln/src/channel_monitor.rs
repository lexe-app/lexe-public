use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::bail;
use common::ln::channel::LxOutPoint;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::chainmonitor::MonitorUpdateId;
use lightning::chain::transaction::OutPoint;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::alias::LexeChainMonitorType;
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::LexePersister;

type BoxedAnyhowFuture =
    Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Represents a channel monitor update. See docs on each field for details.
pub struct LxChannelMonitorUpdate {
    pub funding_txo: LxOutPoint,
    pub update_id: MonitorUpdateId,
    /// A [`Future`] which makes an api call (typically with retries) to the
    /// backend to persist the channel monitor state, returning an
    /// `anyhow::Result<()>` once either (1) persistence succeeds or (2) there
    /// were too many failures to keep trying. We take this future as input
    /// (instead of e.g. a `NodeFile`) because it is the cleanest and easiest
    /// way to abstract over the user node and LSP's differing api clients, vfs
    /// structures, and expected error types.
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
/// This prevent a race conditions where two monitor updates come in quick
/// succession and the newer channel monitor state is overwritten by the older
/// channel monitor state.
pub fn spawn_channel_monitor_persister_task<PS>(
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
    mut channel_monitor_persister_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    process_events_tx: mpsc::Sender<()>,
    test_event_tx: TestEventSender,
    mut shutdown: ShutdownChannel,
) -> LxTask<()>
where
    PS: LexePersister,
{
    debug!("Starting channel monitor persister task");
    LxTask::spawn_named("channel monitor persister", async move {
        let mut idx = 0;
        loop {
            tokio::select! {
                Some(update) = channel_monitor_persister_rx.recv() => {
                    idx += 1;

                    let handle_res = handle_update(
                        chain_monitor.as_ref(),
                        update,
                        idx,
                        &process_events_tx,
                        &test_event_tx,
                        &mut shutdown,
                    ).await;

                    if let Err(e) = handle_res {
                        error!("Channel monitor persist error: {e:#}");
                        // All errors are considered fatal.
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
/// Returning [`Err`] means that a fatal error was reached; the caller should
/// send a shutdown signal and exit.
async fn handle_update<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    update: LxChannelMonitorUpdate,
    idx: usize,
    process_events_tx: &mpsc::Sender<()>,
    test_event_tx: &TestEventSender,
    shutdown: &mut ShutdownChannel,
) -> anyhow::Result<()> {
    debug!("Handling channel monitor update #{idx}");

    // Run the persist future
    let kind = update.kind;
    debug!("Running {kind} channel persist future #{idx}");
    let persist_result = tokio::select! {
        res = update.api_call_fut => res,
        () = shutdown.recv() => bail!("Received shutdown signal"),
    };

    if let Err(e) = persist_result {
        // Channel monitor persistence errors are serious;
        // return err and shut down to prevent any loss of funds.
        bail!("Couldn't persist {kind} channel #{idx}: {e:#}");
    }

    // Update the chain monitor with the update id and funding txo the channel
    // monitor update.
    let chain_monitor_update_res = chain_monitor.channel_monitor_updated(
        OutPoint::from(update.funding_txo),
        update.update_id,
    );
    if let Err(e) = chain_monitor_update_res {
        // If the update wasn't accepted, the channel is disabled, so no
        // transactions can be made. Just return err and shut down.
        bail!("Chain monitor returned err: {e:?}");
    }

    // Trigger the background processor to reprocess events, as the completed
    // channel monitor update may have generated an event that can be handled,
    // such as to restore monitor updating and broadcast a funding tx.
    let _ = process_events_tx.try_send(());

    info!("Success: persisted {kind} channel #{idx}");
    test_event_tx.send(TestEvent::ChannelMonitorPersisted);

    Ok(())
}
