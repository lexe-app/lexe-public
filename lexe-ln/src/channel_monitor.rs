use std::{
    fmt::{self, Display},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use common::{
    ln::channel::LxOutPoint, shutdown::ShutdownChannel, task::LxTask, Apply,
};
use lightning::chain::{chainmonitor::MonitorUpdateId, transaction::OutPoint};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::{alias::LexeChainMonitorType, traits::LexePersister};

/// How long we'll wait to receive a reply from the background processor that
/// event processing is complete.
const PROCESS_EVENTS_TIMEOUT: Duration = Duration::from_secs(15);

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
    /// (instead of e.g. a `VfsFile`) because it is the cleanest and easiest
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
    process_events_tx: mpsc::Sender<oneshot::Sender<()>>,
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
                        &mut shutdown,
                    ).await;

                    if let Err(error) = handle_res {
                        // Log `Interrupted` at `warn` since it is a race that
                        // is expected to occur, especially in integration
                        // tests. Everything else is logged at `error`.
                        match error {
                            Error::Interrupted =>
                                warn!("Monitor persist interrupted"),
                            e => error!("Monitor persist error: {e:#}"),
                        }

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

/// Errors that can occur when handling a channel monitor update.
///
/// This enum is intentionally kept private; it exists solely to prevent the
/// caller from having to use some variant of `err_str.contains(..)`
#[derive(Debug, Error)]
enum Error {
    #[error("Couldn't persist {kind} channel #{idx}: {inner:#}")]
    PersistFailure {
        kind: ChannelMonitorUpdateKind,
        idx: usize,
        inner: anyhow::Error,
    },
    #[error("Chain monitor returned err: {0:?}")]
    ChainMonitor(lightning::util::errors::APIError),
    #[error("Received shutdown signal")]
    Interrupted,
    #[error("Timed out waiting for events to be processed")]
    EventsProcessTimeout,
    #[error("Could not receive reply from the `processed_rx` channel")]
    EventsProcessRecv,
}

/// A helper to prevent [`spawn_channel_monitor_persister_task`]'s control flow
/// from getting too complex.
///
/// Since channel monitor persistence is very important, all [`Err`]s are
/// considered fatal; the caller should send a shutdown signal and exit.
async fn handle_update<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    update: LxChannelMonitorUpdate,
    idx: usize,
    process_events_tx: &mpsc::Sender<oneshot::Sender<()>>,
    shutdown: &mut ShutdownChannel,
) -> Result<(), Error> {
    debug!("Handling channel monitor update #{idx}");

    // Run the persist future
    let kind = update.kind;
    debug!("Running {kind} channel persist future #{idx}");
    let persist_result = tokio::select! {
        res = update.api_call_fut => res,
        () = shutdown.recv() => return Err(Error::Interrupted),
    };

    if let Err(inner) = persist_result {
        // Channel monitor persistence errors are serious;
        // return err and shut down to prevent any loss of funds.
        return Err(Error::PersistFailure { kind, idx, inner });
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
        return Err(Error::ChainMonitor(e));
    }

    // Trigger the background processor to reprocess events, as the completed
    // channel monitor update may have generated an event that can be handled,
    // such as to restore monitor updating and broadcast a funding tx.
    // Furthermore, wait for the event to be handled.
    let (processed_tx, processed_rx) = oneshot::channel();
    let _ = process_events_tx.try_send(processed_tx);
    processed_rx
        .apply(|rx| tokio::time::timeout(PROCESS_EVENTS_TIMEOUT, rx))
        .await
        .map_err(|_| Error::EventsProcessTimeout)?
        .map_err(|_| Error::EventsProcessRecv)?;

    info!("Success: persisted {kind} channel #{idx}");

    Ok(())
}
