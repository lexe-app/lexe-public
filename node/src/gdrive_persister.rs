use anyhow::Context;
use bytes::Bytes;
use futures::future;
use gdrive::GoogleVfs;
use lexe_api::vfs::VfsFileId;
use lexe_std::backoff;
use lexe_tokio::notify_once::NotifyOnce;
use tokio::sync::Semaphore;
use tracing::error;

use crate::backup_persister::BackupBatch;

/// Persist a [`BackupBatch`] of VFS files (writes+deletes) to GDrive.
pub(crate) async fn persist(
    gvfs: &GoogleVfs,
    files: &mut BackupBatch,
    shutdown: &NotifyOnce,
) -> anyhow::Result<()> {
    const MAX_CONCURRENT_WRITES: usize = 16;

    let semaphore = &Semaphore::const_new(MAX_CONCURRENT_WRITES);
    let mut writes = Vec::new();
    let mut deletes = Vec::new();
    for (id, data) in files.drain() {
        match data {
            Some(data) => writes.push(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .expect("Semaphore is never closed");

                let result = helpers::upsert(gvfs, id, data.into()).await;
                if let Err(e) = &result {
                    // Signal now, even while other writes are still running.
                    error!("GDrive write failed, shutting down: {e:#}");
                    shutdown.send();
                }
                result
            }),
            None => deletes.push(id),
        }
    }

    for result in future::join_all(writes).await {
        result?;
    }

    // GDrive has no atomic write+delete batch, so finish writes before deletes.
    for id in deletes {
        if gvfs.file_exists(&id).await {
            gvfs.delete_file(&id)
                .await
                .with_context(|| format!("Couldn't delete GDrive file {id}"))?;
        }
    }
    Ok(())
}

mod helpers {
    use super::*;

    pub(super) async fn upsert(
        gvfs: &GoogleVfs,
        id: VfsFileId,
        data: Bytes,
    ) -> anyhow::Result<()> {
        const RETRIES: usize = 5;
        let mut backoff = backoff::get_backoff_iter().take(RETRIES);
        loop {
            let error = match gvfs.upsert_file(&id, data.clone()).await {
                Ok(()) => return Ok(()),
                Err(error) => error,
            };
            let Some(delay) = backoff.next() else {
                return Err(error).with_context(|| {
                    format!("Couldn't upsert GDrive file {id}")
                });
            };
            tokio::time::sleep(delay).await;
        }
    }
}
