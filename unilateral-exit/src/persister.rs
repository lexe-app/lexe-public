//! A [`Vfs`] over a local directory, replacing Lexe's DB for the recovery
//! node. Files live at `<recovery_dir>/<dirname>/<filename>` and hold the same
//! ciphertext the usernode persists, so the recovery dir is as secure at rest
//! as the user's Google Drive backup.
//!
//! [`Vfs`]: lexe_api::vfs::Vfs

use std::{io, path::PathBuf, sync::Arc};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use lexe_api::{
    error::BackendApiError,
    types::{Empty, retries::Retries},
    vfs::{
        self, UpsertOrDeleteFileBatchRequest, Vfs, VfsDirectory,
        VfsDirectoryList, VfsFile, VfsFileId,
    },
};
use lexe_common::ln::channel::{ChannelId, LxMonitorName};
use lexe_crypto::{aes::AesMasterKey, rng::SysRng};
use lexe_ln::{
    alias::{ChannelMonitorType, LexeChainMonitorType, SignerType},
    channel_monitor::{
        self, ChannelMonitorPersisterCommand, ChannelMonitorUpdateKind,
    },
    persister,
    traits::LexePersister,
    wallet::ChangeSet,
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::{
    chain::{
        ChannelMonitorUpdateStatus, chainmonitor::Persist,
        channelmonitor::ChannelMonitorUpdate,
    },
    util::{persist::MonitorName, ser::Writeable},
};
use serde::Serialize;
use tokio::{io::AsyncWriteExt, sync::mpsc};
use tracing::{debug, info, info_span, warn};

const CHANNEL_MONITOR_PERSISTER_BACKLOG_WARN_THRESHOLD: usize = 10;

pub struct RecoveryPersister {
    channel_monitor_persister_tx: mpsc::Sender<ChannelMonitorPersisterCommand>,
    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    recovery_dir: PathBuf,
    shutdown: NotifyOnce,
    vfs_master_key: Arc<AesMasterKey>,
}

impl RecoveryPersister {
    pub(crate) fn new(
        channel_monitor_persister_tx: mpsc::Sender<
            ChannelMonitorPersisterCommand,
        >,
        eph_tasks_tx: mpsc::Sender<LxTask<()>>,
        recovery_dir: PathBuf,
        shutdown: NotifyOnce,
        vfs_master_key: Arc<AesMasterKey>,
    ) -> Self {
        Self {
            channel_monitor_persister_tx,
            eph_tasks_tx,
            recovery_dir,
            shutdown,
            vfs_master_key,
        }
    }

    pub(crate) async fn read_wallet_changeset(
        &self,
    ) -> anyhow::Result<Option<ChangeSet>> {
        let file_id = VfsFileId::new(
            vfs::SINGLETON_DIRECTORY,
            vfs::WALLET_CHANGESET_V2_FILENAME,
        );
        self.read_json::<ChangeSet>(&file_id).await
    }

    /// Returns the local path backing a VFS file, refusing path traversal.
    fn file_path(
        &self,
        file_id: &VfsFileId,
    ) -> Result<PathBuf, BackendApiError> {
        let dirname: &str = file_id.dir.dirname.as_ref();
        let filename: &str = file_id.filename.as_ref();
        for component in [dirname, filename] {
            // Allow `.`, the singleton dirname, but reject any other component
            // that could escape the recovery dir.
            let ok = component == vfs::SINGLETON_DIRECTORY
                || !(component.is_empty()
                    || component.contains(['/', '\\'])
                    || component == "..");
            if !ok {
                return Err(BackendApiError::invalid_parsed_req(format!(
                    "Invalid VFS path component: {component}"
                )));
            }
        }
        Ok(self.recovery_dir.join(dirname).join(filename))
    }
}

#[async_trait]
impl Vfs for RecoveryPersister {
    async fn get_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Option<VfsFile>, BackendApiError> {
        let path = self.file_path(file_id)?;
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(Some(VfsFile {
                id: file_id.clone(),
                data,
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(helpers::io_to_backend_err(&path, e)),
        }
    }

    async fn upsert_file(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        _retries: Retries,
    ) -> Result<Empty, BackendApiError> {
        let path = self.file_path(file_id)?;
        let parent = path.parent().expect("Always under recovery_dir");
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| helpers::io_to_backend_err(parent, e))?;

        // Write to a temp file, fsync, then atomically rename into place so a
        // crash can't leave a torn file.
        let tmp_path = path.with_extension("tmp");
        let mut tmp_file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| helpers::io_to_backend_err(&tmp_path, e))?;
        tmp_file
            .write_all(&data)
            .await
            .map_err(|e| helpers::io_to_backend_err(&tmp_path, e))?;
        tmp_file
            .sync_all()
            .await
            .map_err(|e| helpers::io_to_backend_err(&tmp_path, e))?;
        drop(tmp_file);
        tokio::fs::rename(&tmp_path, &path)
            .await
            .map_err(|e| helpers::io_to_backend_err(&path, e))?;

        Ok(Empty {})
    }

    async fn delete_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Empty, BackendApiError> {
        let path = self.file_path(file_id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(Empty {}),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Empty {}),
            Err(e) => Err(helpers::io_to_backend_err(&path, e)),
        }
    }

    async fn upsert_or_delete_file_batch(
        &self,
        _req: UpsertOrDeleteFileBatchRequest,
    ) -> Result<Empty, BackendApiError> {
        Err(BackendApiError::database(
            "Batch writes are unsupported: the recovery dir can't write \
             multiple files atomically",
        ))
    }

    async fn list_directory(
        &self,
        dir: &VfsDirectory,
    ) -> Result<VfsDirectoryList, BackendApiError> {
        let path = self.recovery_dir.join(dir.dirname.as_ref());
        let mut filenames = Vec::new();
        let mut read_dir = match tokio::fs::read_dir(&path).await {
            Ok(read_dir) => read_dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound =>
                return Ok(VfsDirectoryList {
                    dirname: dir.dirname.clone(),
                    filenames,
                }),
            Err(e) => return Err(helpers::io_to_backend_err(&path, e)),
        };
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| helpers::io_to_backend_err(&path, e))?
        {
            let filename = entry.file_name().to_string_lossy().into_owned();
            // Skip temp files left by a crashed `upsert_file`.
            if filename.ends_with(".tmp") {
                continue;
            }
            filenames.push(filename);
        }
        Ok(VfsDirectoryList {
            dirname: dir.dirname.clone(),
            filenames,
        })
    }

    fn encrypt_json<T: Serialize>(
        &self,
        file_id: VfsFileId,
        value: &T,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_json(&mut rng, &self.vfs_master_key, file_id, value)
    }

    fn encrypt_ldk_writeable<W: Writeable>(
        &self,
        file_id: VfsFileId,
        writeable: &W,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_ldk_writeable(
            &mut rng,
            &self.vfs_master_key,
            file_id,
            writeable,
        )
    }

    fn encrypt_bytes(
        &self,
        file_id: VfsFileId,
        plaintext_bytes: &[u8],
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_bytes(
            &mut rng,
            &self.vfs_master_key,
            file_id,
            plaintext_bytes,
        )
    }

    fn decrypt_file(
        &self,
        expected_file_id: &VfsFileId,
        file: VfsFile,
    ) -> anyhow::Result<Vec<u8>> {
        persister::decrypt_file(&self.vfs_master_key, expected_file_id, file)
    }
}

#[async_trait]
impl persister::LightningPersisterMethods for RecoveryPersister {
    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");
        let file_id = VfsFileId::new(
            vfs::SINGLETON_DIRECTORY,
            vfs::CHANNEL_MANAGER_FILENAME,
        );
        let file = self.encrypt_ldk_writeable(file_id, channel_manager);
        let retries = Retries::IMPORTANT_PERSISTS;
        self.persist_file(file, retries)
            .await
            .context("Failed to persist channel manager")
    }

    async fn persist_channel_monitor<PS: LexePersister>(
        &self,
        chain_monitor: &LexeChainMonitorType<PS>,
        channel_id: &ChannelId,
        monitor_name: &LxMonitorName,
    ) -> anyhow::Result<()> {
        let file = {
            let locked_monitor = chain_monitor
                .get_monitor(lightning::ln::types::ChannelId::from(*channel_id))
                .map_err(|e| {
                    anyhow!("No monitor for this channel_id: {e:?}")
                })?;
            let filename = monitor_name.to_string();
            let file_id = VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, filename);
            self.encrypt_ldk_writeable(file_id, &*locked_monitor)
        };
        let retries = Retries::IMPORTANT_PERSISTS;
        self.persist_file(file, retries)
            .await
            .context("Failed to persist channel monitor")
    }

    fn queue_backup(&self, _file: &VfsFile) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Persist<SignerType> for RecoveryPersister {
    fn persist_new_channel(
        &self,
        name: MonitorName,
        monitor: &ChannelMonitorType,
    ) -> ChannelMonitorUpdateStatus {
        let update = None;
        channel_monitor::queue_monitor_persist(
            &self.channel_monitor_persister_tx,
            &self.shutdown,
            CHANNEL_MONITOR_PERSISTER_BACKLOG_WARN_THRESHOLD,
            ChannelMonitorUpdateKind::New,
            name,
            update,
            monitor,
        )
    }

    fn update_persisted_channel(
        &self,
        name: MonitorName,
        update: Option<&ChannelMonitorUpdate>,
        monitor: &ChannelMonitorType,
    ) -> ChannelMonitorUpdateStatus {
        channel_monitor::queue_monitor_persist(
            &self.channel_monitor_persister_tx,
            &self.shutdown,
            CHANNEL_MONITOR_PERSISTER_BACKLOG_WARN_THRESHOLD,
            ChannelMonitorUpdateKind::Updated,
            name,
            update,
            monitor,
        )
    }

    fn archive_persisted_channel(&self, name: MonitorName) {
        let monitor_name = LxMonitorName::from(name);
        let eph_tasks_tx = self.eph_tasks_tx.clone();
        let recovery_dir = self.recovery_dir.clone();
        let vfs_master_key = self.vfs_master_key.clone();

        let try_archive_fut = async move {
            let filename = monitor_name.to_string();
            info!(monitor = filename, "Archiving channel monitor");

            let source_file_id =
                VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, filename.clone());
            let archive_file_id =
                VfsFileId::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, filename);

            // `archive_persisted_channel`'s `&self` isn't `'static`, so
            // reconstruct a persister for the spawned task.
            let (unused_tx, _unused_rx) = mpsc::channel(1);
            let persister = RecoveryPersister::new(
                unused_tx,
                eph_tasks_tx,
                recovery_dir,
                NotifyOnce::new(),
                vfs_master_key,
            );

            // The ciphertext is bound to its path, so rather than moving the
            // file, decrypt it and re-encrypt it under the archive path.
            let source_plaintext = persister
                .read_bytes(&source_file_id)
                .await
                .context("Couldn't read source monitor")?
                .context("No source monitor exists")?;

            let archive_file =
                persister.encrypt_bytes(archive_file_id, &source_plaintext);
            let retries = Retries::from_count(3);
            persister
                .persist_file(archive_file, retries)
                .await
                .context("Failed to upsert archive file")?;

            persister
                .remove_file(&source_file_id)
                .await
                .context("Couldn't delete archived channel monitor")?;

            anyhow::Ok(())
        };

        const SPAN_NAME: &str = "(chan-monitor-archiver)";
        let task =
            LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async {
                match try_archive_fut.await {
                    Ok(()) => info!("Success: archived channel monitor"),
                    Err(e) => warn!("Couldn't archive monitor: {e:#}"),
                }
            });
        let _ = self.eph_tasks_tx.try_send(task);
    }
}

mod helpers {
    use super::*;

    pub(super) fn io_to_backend_err(
        path: &std::path::Path,
        e: io::Error,
    ) -> BackendApiError {
        BackendApiError::database(format!("{}: {e}", path.display()))
    }
}
