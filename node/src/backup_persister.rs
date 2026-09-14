//! An actor for asynchronously backing up encrypted VFS files to a single
//! remote `BackupStore`.

use std::{collections::HashMap, time::Duration};

use gdrive::GoogleVfs;
use lexe_api::vfs::VfsFileId;
use lexe_ln::persister::BackupCommand;
use lexe_tokio::{DEFAULT_CHANNEL_SIZE, notify_once::NotifyOnce, task::LxTask};
use tokio::{sync::mpsc, time};
use tracing::{debug, error, info_span};

use crate::gdrive_persister;

#[cfg(test)]
mod tests;

/// A `BackupPersister` accepts [`BackupCommand`] requests, batches writes, and
/// then updates its remote backup `store`.
pub(crate) struct BackupPersister {
    store: BackupStore,
    rx: mpsc::Receiver<BackupCommand>,
    pending: BackupBatch,
}

pub(crate) enum BackupStore {
    GDrive(Box<GoogleVfs>),
    #[cfg(test)]
    Test(tests::TestStore),
}

/// A write batch that's pending backup. `None` means delete the file.
pub(crate) type BackupBatch = HashMap<VfsFileId, Option<Vec<u8>>>;

impl BackupPersister {
    const PERSIST_DELAY: Duration = Duration::from_secs(60);

    pub(crate) fn new(
        store: BackupStore,
    ) -> (Self, mpsc::Sender<BackupCommand>) {
        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let persister = Self {
            store,
            rx,
            pending: HashMap::new(),
        };
        (persister, tx)
    }

    /// Spawn the `BackupPersister` task.
    ///
    /// - `backup_persister_shutdown`: `ChannelMonitorPersister` triggers
    ///   `BackupPersister` shutdown only after it's done shutting down and all
    ///   monitor persists are flushed+enqueued.
    /// - `shutdown`: Used to trigger a node shutdown if backup fails.
    pub(crate) fn spawn(
        self,
        backup_persister_shutdown: NotifyOnce,
        shutdown: NotifyOnce,
    ) -> LxTask<()> {
        const SPAN_NAME: &str = "(backup-persister)";
        let span = info_span!(SPAN_NAME, store = self.store.name());
        LxTask::spawn_with_span(
            SPAN_NAME,
            span,
            self.run(backup_persister_shutdown, shutdown),
        )
    }

    async fn run(
        mut self,
        mut backup_persister_shutdown: NotifyOnce,
        shutdown: NotifyOnce,
    ) {
        let mut commands = Vec::with_capacity(DEFAULT_CHANNEL_SIZE);
        let delay_timer = time::sleep(Self::PERSIST_DELAY);
        tokio::pin!(delay_timer);

        loop {
            tokio::select! {
                num_commands = self.rx.recv_many(
                    &mut commands,
                    DEFAULT_CHANNEL_SIZE,
                ) => {
                    // All txs closed -> shutdown
                    if num_commands == 0 { break; }

                    // Start timer for when we'll flush this batch
                    if self.pending.is_empty() {
                        delay_timer.as_mut().reset(
                            time::Instant::now() + Self::PERSIST_DELAY,
                        );
                    }

                    // Accumulate writes into pending batch
                    self.handle_commands(&mut commands);
                }

                // Delay timer triggered -> flush pending batch
                () = &mut delay_timer, if !self.pending.is_empty() => {
                    self.flush(&shutdown).await;
                }

                () = backup_persister_shutdown.recv() => break,
            }
        }

        // Shutdown. Poll last writes then do final flush.
        self.rx.close();
        while self.rx.recv_many(&mut commands, DEFAULT_CHANNEL_SIZE).await > 0 {
            self.handle_commands(&mut commands);
        }
        self.flush(&shutdown).await;
    }

    /// Accumulate writes into `self.pending` batch.
    fn handle_commands(&mut self, commands: &mut Vec<BackupCommand>) {
        for command in commands.drain(..) {
            match command {
                BackupCommand::Persist(file) => {
                    self.pending.insert(file.id, Some(file.data));
                }
                BackupCommand::Archive {
                    source_file_id,
                    archive_file,
                } => {
                    self.pending.insert(source_file_id, None);
                    self.pending
                        .insert(archive_file.id, Some(archive_file.data));
                }
            }
        }
    }

    /// Flush full pending write batch to remote backup store. If backup fails,
    /// trigger node shutdown.
    async fn flush(&mut self, shutdown: &NotifyOnce) {
        if self.pending.is_empty() {
            return;
        }

        let count = self.pending.len();
        match self.store.persist(&mut self.pending, shutdown).await {
            Ok(()) => debug!(count, "Successful backup"),
            Err(e) => {
                error!("FATAL: Backup failed, shutting down: {e:#}");
                shutdown.send();
            }
        }
    }
}

impl BackupStore {
    fn name(&self) -> &'static str {
        match self {
            Self::GDrive(_) => "GDrive",
            #[cfg(test)]
            Self::Test(_) => "Test",
        }
    }

    /// Drains `files`, retaining its allocation even on failure.
    async fn persist(
        &self,
        files: &mut BackupBatch,
        shutdown: &NotifyOnce,
    ) -> anyhow::Result<()> {
        match self {
            Self::GDrive(gvfs) =>
                gdrive_persister::persist(gvfs, files, shutdown).await,
            #[cfg(test)]
            Self::Test(store) => store.persist(files).await,
        }
    }
}
