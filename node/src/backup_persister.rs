//! An actor for asynchronously backing up encrypted VFS files to a single
//! remote `BackupStore`.

use std::{collections::HashMap, time::Duration};

use gdrive::GoogleVfs;
use lexe_api::vfs::VfsFileId;
use lexe_ln::persister::BackupCommand;
use lexe_tokio::{DEFAULT_CHANNEL_SIZE, notify_once::NotifyOnce, task::LxTask};
use tokio::{sync::mpsc, time};
use tracing::{debug, error, info_span};

use crate::{gdrive_persister, vss_persister::VssPersister};

#[cfg(test)]
mod tests;

/// A `BackupPersister` accepts [`BackupCommand`] requests, batches writes, and
/// then updates its remote backup `store`.
pub(crate) struct BackupPersister {
    store: BackupStore,
    rx: mpsc::Receiver<BackupCommand>,
    pending: BackupBatch,
    shutdown: NotifyOnce,
}

pub(crate) enum BackupStore {
    GDrive(Box<GoogleVfs>),
    Vss(VssPersister),
    #[cfg(test)]
    Test(tests::TestStore),
}

/// A write batch that's pending backup. `None` means delete the file.
pub(crate) type BackupBatch = HashMap<VfsFileId, Option<Vec<u8>>>;

impl BackupPersister {
    const PERSIST_DELAY: Duration = Duration::from_secs(60);

    /// The channel monitor persister triggers `shutdown` after flushing and
    /// enqueuing all monitor persists.
    pub(crate) fn new(
        store: BackupStore,
        shutdown: NotifyOnce,
    ) -> (Self, mpsc::Sender<BackupCommand>) {
        let (tx, rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let persister = Self {
            store,
            rx,
            pending: HashMap::new(),
            shutdown,
        };
        (persister, tx)
    }

    /// Spawn the `BackupPersister` task.
    pub(crate) fn spawn(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(backup-persister)";
        let span = info_span!(SPAN_NAME, store = self.store.name());
        LxTask::spawn_with_span(SPAN_NAME, span, self.run())
    }

    async fn run(mut self) {
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
                    self.flush().await;
                }

                () = self.shutdown.recv() => break,
            }
        }

        // Shutdown. Poll last writes then do final flush.
        self.rx.close();
        while self.rx.recv_many(&mut commands, DEFAULT_CHANNEL_SIZE).await > 0 {
            self.handle_commands(&mut commands);
        }
        self.flush().await;
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

    /// Flush full pending write batch to remote backup store.
    async fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }

        let count = self.pending.len();
        match self.store.persist(&mut self.pending).await {
            Ok(()) => debug!(count, "Successful backup"),
            Err(e) => error!("Backup failed: {e:#}"),
        }
    }
}

impl BackupStore {
    fn name(&self) -> &'static str {
        match self {
            Self::GDrive(_) => "GDrive",
            Self::Vss(_) => "VSS",
            #[cfg(test)]
            Self::Test(_) => "Test",
        }
    }

    /// Drains `files`, retaining its allocation even on failure.
    async fn persist(&self, files: &mut BackupBatch) -> anyhow::Result<()> {
        match self {
            Self::GDrive(gvfs) => gdrive_persister::persist(gvfs, files).await,
            Self::Vss(vss) => vss.persist(files).await,
            #[cfg(test)]
            Self::Test(store) => store.persist(files).await,
        }
    }
}
