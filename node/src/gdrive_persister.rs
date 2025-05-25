use std::{collections::HashMap, sync::Arc, time::Duration};

use common::constants;
use lexe_api::vfs::{VfsFile, VfsFileId};
use lexe_std::Apply;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::sync::mpsc;
use tracing::{error, info, info_span};

use crate::persister::NodePersister;

/// Whenever we receive a file to persist, we'll wait at least this long before
/// we begin persisting so we are likely to batch multiple persists together.
///
/// Payments (with retries) can sometimes take up to a minute, so we'll wait at
/// least a minute before persisting, unless a shutdown signal was received.
const PERSIST_DELAY: Duration = Duration::from_secs(60);

/// Spawns a task which asynchronously persists critical files to GDrive
/// (generally just the channel manager and channel monitors).
///
/// # Shutdown
///
/// This task can be triggered by the channel monitor persister task:
///
/// 1) Channel monitor update arrives to monitor persister task
///    - Monitor persister calls `persister.persist_channel_monitor()`
///    - Sends an update over `gdrive_persister_tx`
///
/// 2) Channel monitor persister task is shutting down
///    - `channel_manager.get_and_clear_needs_persistence()` returns true
///    - Calls `persister.persist_manager()`
///    - Sends an update over `gdrive_persister_tx`
///
/// Thus, this task must live at least as long as the monitor persister task. So
/// the monitor persister task will trigger `gdrive_persister_shutdown` only
/// once it's completed its own shutdown sequence.
pub fn spawn_gdrive_persister_task(
    persister: Arc<NodePersister>,
    mut gdrive_persister_rx: mpsc::Receiver<VfsFile>,
    mut gdrive_persister_shutdown: NotifyOnce,
    shutdown: NotifyOnce,
) -> LxTask<()> {
    const SPAN_NAME: &str = "(gdrive-persister)";
    LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
        let mut persist_queue: HashMap<VfsFileId, VfsFile> = HashMap::new();

        let delay_timer: Option<tokio::time::Sleep> = None;
        tokio::pin!(delay_timer);

        loop {
            tokio::select! {
                Some(file) = gdrive_persister_rx.recv() => {
                    // Start the timer if it hasn't already been started.
                    if delay_timer.is_none() {
                        delay_timer.set(Some(tokio::time::sleep(PERSIST_DELAY)));
                    }

                    // Save the file to the persist queue,
                    // overwriting any existing entries.
                    persist_queue.insert(file.id.clone(), file);
                }

                // This fut resolves if the delay timer exists and is completed.
                // The `async` block ensures that the future is *constructed*
                // lazily (even though it isn't polled), otherwise we panic.
                // https://github.com/tokio-rs/tokio/issues/2583#issuecomment-638212772
                _ = async { delay_timer.as_mut().as_pin_mut().unwrap().await },
                    if delay_timer.is_some() => {
                    delay_timer.set(None);

                    drain_persist_queue(
                        &mut persist_queue, &persister, &shutdown
                    ).await;
                }

                () = gdrive_persister_shutdown.recv() => break,
            }
        }

        // Ensure all pending persists have been drained.
        drain_persist_queue(&mut persist_queue, &persister, &shutdown).await;
    })
}

/// Drains the `persist_queue` by persisting all files in the queue to GDrive.
async fn drain_persist_queue(
    persist_queue: &mut HashMap<VfsFileId, VfsFile>,
    persister: &NodePersister,
    shutdown: &NotifyOnce,
) {
    if persist_queue.is_empty() {
        return;
    }

    // Alternative non-concurrent implementation
    /*
    let retries = constants::IMPORTANT_PERSIST_RETRIES;
    for (file_id, file) in persist_queue.drain() {
        match persister.upsert_gdrive_if_available(file, retries).await {
            Ok(()) => info!("Successful backup to GDrive: {file_id}"),
            Err(e) => {
                error!("FATAL: Backup to GDrive failed, shutting down: {e:#}");
                shutdown.send();
            }
        }
    }
    */

    persist_queue
        .drain()
        .map(|(file_id, file)| async move {
            let retries = constants::IMPORTANT_PERSIST_RETRIES;
            match persister.upsert_gdrive_if_available(file, retries).await {
                Ok(()) => info!("Successful backup to GDrive: {file_id}"),
                Err(e) => {
                    // Since we're persisting critical state, if persist fails,
                    // just shut down the node.
                    error!("FATAL: GDrive backup failed, shutting down: {e:#}");
                    shutdown.send();
                }
            }
        })
        .apply(futures::future::join_all)
        .await;
}
