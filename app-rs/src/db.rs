use std::{io, sync::Arc, time::Duration};

use anyhow::Context;
use common::debug_panic_release_log;
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
use serde::{Deserialize, Serialize, de};
use tracing::info;

use crate::ffs::Ffs;

/// A generic write-back database for the app.
///
/// Persistence is currently done asynchronously out-of-band, so calling
/// [`WritebackDb::update`] only modifies the in-memory state. The
/// [`DbPersister`] will finish writing the data to durable storage
/// later (by at most 500ms).
pub(crate) struct WritebackDb<D> {
    /// The current in-memory db data.
    db: Arc<std::sync::Mutex<D>>,
    /// Notify the [`DbPersister`] to persist the db data.
    persist_tx: notify::Sender,
    /// Handle to spawned [`DbPersister`].
    persist_task: Option<LxTask<()>>,
    /// Trigger shutdown of [`DbPersister`].
    shutdown: NotifyOnce,
}

impl<D> WritebackDb<D>
where
    D: Sized
        + Serialize
        + for<'de> de::Deserialize<'de>
        + Default
        + Send
        + 'static
        + Clone
        + Update,
{
    pub(crate) fn load<F: Ffs + Send + 'static>(
        ffs: F,
        filename: &str,
        task_name: &str,
    ) -> Self {
        let db =
            Arc::new(std::sync::Mutex::new(DbPersister::load(&ffs, filename)));
        let (persist_tx, persist_rx) = notify::channel();
        let shutdown = NotifyOnce::new();

        // spawn a task that we can notify to write updates to durable
        // storage.
        let persister = DbPersister::new(
            ffs,
            filename.to_owned(),
            db.clone(),
            persist_rx,
            shutdown.clone(),
        );
        let persist_task =
            Some(LxTask::spawn(task_name.to_owned(), persister.run()));

        Self {
            db,
            persist_tx,
            persist_task,
            shutdown,
        }
    }

    /// Shutdown the [`WritebackDb`]. Flushes any pending writes to disk.
    // TODO(phlip9): remove `dead_code` when we can actually hook the applicaton
    // lifecycle properly.
    #[allow(dead_code)]
    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        // Trigger task to shutdown.
        self.shutdown.send();

        // Wait for task to finish (with timeout).
        let persist_task =
            self.persist_task.take().context("Called shutdown twice")?;
        tokio::time::timeout(Duration::from_secs(1), persist_task)
            .await
            .context("db persister failed to shutdown in time")?
            .context("db persister panicked")
    }

    /// Return a clone of the current in-memory `D` value.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn read(&self) -> D {
        self.db.lock().unwrap().clone()
    }

    /// Reset the in-memory `D` to its default value and notify the
    /// [`DbPersister`].
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn reset(&self) {
        *self.db.lock().unwrap() = D::default();
        self.persist_tx.send();
    }

    /// Update the in-memory `D` by merging in any `Some` fields in
    /// `update`. Then notify the [`DbPersister`] that we need to save,
    /// but don't wait for it to actually persist.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn update(&self, update: D) -> anyhow::Result<()> {
        self.db.lock().unwrap().update(update)?;
        self.persist_tx.send();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn db(&self) -> &Arc<std::sync::Mutex<D>> {
        &self.db
    }
}

/// Persists data asynchronously when notified by the [`WritebackDb`].
pub(crate) struct DbPersister<F, D> {
    /// Data flat file store.
    ffs: F,
    /// Filename to persist to.
    filename: String,
    /// The current in-memory db data.
    db: Arc<std::sync::Mutex<D>>,
    /// Receives notifications when the db have updated.
    persist_rx: notify::Receiver,
    /// Receives shutdown signal.
    shutdown: NotifyOnce,
}

impl<F, D> DbPersister<F, D>
where
    F: Ffs,
    D: Sized + Serialize + for<'de> de::Deserialize<'de> + Default,
{
    pub(crate) fn new(
        ffs: F,
        filename: String,
        db: Arc<std::sync::Mutex<D>>,
        persist_rx: notify::Receiver,
        shutdown: NotifyOnce,
    ) -> Self {
        Self {
            ffs,
            filename,
            db,
            persist_rx,
            shutdown,
        }
    }

    pub(crate) async fn run(mut self) {
        loop {
            // Wait for persist notification (or shutdown).
            tokio::select! {
                () = self.persist_rx.recv() => (),
                () = self.shutdown.recv() => break,
            }

            // Read and serialize the current db, then write to ffs.
            self.do_persist().await;

            // Rate-limit persists to at-most once per 500ms
            if let Ok(()) = tokio::time::timeout(
                Duration::from_millis(500),
                self.shutdown.recv(),
            )
            .await
            {
                // Ok => "shutdown.recv()" before timeout
                break;
            }
        }

        // Do a final flush on shutdown if there's any work to be done.
        if self.persist_rx.try_recv() {
            self.do_persist().await;
        }

        info!("persister {}: complete", self.filename);
    }

    pub(crate) async fn do_persist(&mut self) {
        if let Err(err) = self.do_persist_inner().await {
            debug_panic_release_log!(
                "Error persisting {}: {err:#}",
                self.filename
            );
        }
    }

    async fn do_persist_inner(&mut self) -> anyhow::Result<()> {
        // Only hold the lock long enough to serialize
        let db_json_bytes = {
            let db_json = self.db.lock().unwrap();
            Self::serialize_json(&db_json)?
        };
        self.ffs
            .write(self.filename.as_str(), &db_json_bytes)
            .context(format!("Failed to write {} file", self.filename))?;

        Ok(())
    }

    /// Load data from the json file. Resets to default db data if
    /// something goes wrong.
    pub(crate) fn load(ffs: &F, filename: &str) -> D {
        match Self::load_from_file(ffs, filename) {
            Ok(Some(db)) => db,
            Ok(None) => D::default(),
            Err(err) => {
                debug_panic_release_log!("failed to load: {err:#}");
                D::default()
            }
        }
        // TODO(phlip9): run migrations if db.version != Version::current
    }

    fn load_from_file(ffs: &F, filename: &str) -> anyhow::Result<Option<D>> {
        let buf = match ffs.read(filename) {
            Ok(buf) => buf,
            Err(err) if err.kind() == io::ErrorKind::NotFound =>
                return Ok(None),
            Err(err) => return Err(err).context("Failed to read {filename}"),
        };
        let data = Self::deserialize_json(&buf)?;
        Ok(Some(data))
    }

    pub(crate) fn serialize_json(db: &D) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec_pretty(db).context("Failed to serialize {filename}")
    }

    pub(crate) fn deserialize_json(s: &[u8]) -> anyhow::Result<D> {
        serde_json::from_slice(s).context("Failed to deserialize file")
    }
}

/// Trait for merging updates into a database.
pub trait Update: Sized {
    /// Merge updated db data from `update` into `self`.
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        // Default impl for "Atom" types, where updates jus replace `self` and
        // don't traverse.
        *self = update;
        Ok(())
    }
}
impl Update for String {}
impl Update for bool {}

impl<T: Update> Update for Option<T> {
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        match update {
            None => {}
            Some(u) => match self {
                None => *self = Some(u),
                Some(s) => s.update(u)?,
            },
        }
        Ok(())
    }
}

/// General db schema version. Used to determine whether to run migrations.
#[derive(Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(transparent)]
pub(crate) struct SchemaVersion(pub u32);

#[cfg(test)]
mod arb {
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;

    impl Arbitrary for SchemaVersion {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            proptest::prop_oneof![
                10 => Just(Self(1)),
                1 => (0_u32..10).prop_map(Self),
            ]
            .boxed()
        }
    }
}
