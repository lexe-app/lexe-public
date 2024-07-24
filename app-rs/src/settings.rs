//! App settings db, serialization, and persistence.

use std::{io, sync::Arc, time::Duration};

use anyhow::{ensure, Context};
use common::{
    api::fiat_rates::IsoCurrencyCode, notify, shutdown::ShutdownChannel,
    task::LxTask,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::ffs::Ffs;

const SETTINGS_JSON: &str = "settings.json";

/// The app settings DB. Responsible for managing access to the settings.
pub(crate) struct SettingsDb {
    /// The current in-memory settings.
    settings: Arc<std::sync::Mutex<Settings>>,
    /// Notify the [`SettingsPersister`] to persist the settings.
    persist_tx: notify::Sender,
    /// Handle to spawned [`SettingsPersister`].
    persist_task: Option<LxTask<()>>,
    /// Trigger shutdown of [`SettingsPersister`].
    shutdown: ShutdownChannel,
}

/// Persists settings asynchronously, out-of-band.
struct SettingsPersister<F> {
    /// Settings flat file store.
    ffs: F,
    /// The current in-memory settings.
    settings: Arc<std::sync::Mutex<Settings>>,
    /// Receives notifications when the settings have updated.
    persist_rx: notify::Receiver,
    /// Receives shutdown signal.
    shutdown: ShutdownChannel,
}

/// In-memory app settings state.
#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct Settings {
    /// Settings schema version.
    pub schema: SchemaVersion,
    /// Preferred locale.
    pub locale: Option<String>,
    /// Perferred fiat currency (e.g. "USD").
    pub fiat_currency: Option<IsoCurrencyCode>,
}

/// Settings schema version. Used to determine whether to run migrations.
#[derive(Copy, Clone, Eq, PartialEq, Deserialize, Serialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(transparent)]
pub(crate) struct SchemaVersion(pub u32);

// --- impl SettingsDb --- //

impl SettingsDb {
    #[allow(dead_code)] // TODO(phlip9): remove
    pub(crate) fn load<F: Ffs + Send + 'static>(ffs: F) -> Self {
        let settings = Arc::new(std::sync::Mutex::new(Settings::load(&ffs)));
        let (persist_tx, persist_rx) = notify::channel();
        let shutdown = ShutdownChannel::new();
        let persister = SettingsPersister::new(
            ffs,
            settings.clone(),
            persist_rx,
            shutdown.clone(),
        );
        let persist_task =
            Some(LxTask::spawn_named("settings_persist", persister.run()));
        Self {
            settings,
            persist_tx,
            persist_task,
            shutdown,
        }
    }

    #[allow(dead_code)] // TODO(phlip9): remove
    pub(crate) async fn shutdown(&mut self) -> anyhow::Result<()> {
        // Trigger task to shutdown.
        self.shutdown.send();

        // Wait for task to finish (with timeout).
        let persist_task =
            self.persist_task.take().context("Called shutdown twice")?;
        tokio::time::timeout(Duration::from_secs(1), persist_task)
            .await
            .context("settings persister failed to shutdown in time")?
            .context("settings persister panicked")
    }

    #[allow(dead_code)] // TODO(phlip9): remove
    pub(crate) fn update(&self, update: Settings) -> anyhow::Result<()> {
        self.settings.lock().unwrap().update(update)?;
        self.persist_tx.send();
        Ok(())
    }
}

// --- impl SettingsPersister --- //

impl<F> SettingsPersister<F>
where
    F: Ffs,
{
    fn new(
        ffs: F,
        settings: Arc<std::sync::Mutex<Settings>>,
        persist_rx: notify::Receiver,
        shutdown: ShutdownChannel,
    ) -> Self {
        Self {
            ffs,
            settings,
            persist_rx,
            shutdown,
        }
    }

    async fn run(mut self) {
        loop {
            // Wait for persist notification (or shutdown).
            tokio::select! {
                () = self.persist_rx.recv() => (),
                () = self.shutdown.recv() => break,
            }

            // Read and serialize the current settings, then write to ffs.
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

        info!("settings persister: complete");
    }

    async fn do_persist(&mut self) {
        if let Err(err) = self.do_persist_inner().await {
            // Just log the error
            error!("Error persisting settings: {err:#}");
        }
    }

    async fn do_persist_inner(&mut self) -> anyhow::Result<()> {
        // Only hold the lock long enough to serialize
        let settings_json_bytes =
            self.settings.lock().unwrap().serialize_json()?;

        self.ffs
            .write(SETTINGS_JSON, &settings_json_bytes)
            .context("Failed to write settings.json file")?;

        Ok(())
    }
}

// --- impl Settings --- //

impl Settings {
    /// Load settings from settings.json file. Resets to default settings if
    /// something goes wrong.
    fn load<F: Ffs>(ffs: &F) -> Self {
        match Self::load_from_file(ffs) {
            Ok(Some(settings)) => settings,
            Ok(None) => Settings::default(),
            Err(err) => {
                error!("settings: failed to load: {err:#}");
                Settings::default()
            }
        }
        // TODO(phlip9): run migrations if settings.version != Version::current
    }

    /// Try to load settings from settings.json file.
    fn load_from_file<F: Ffs>(ffs: &F) -> anyhow::Result<Option<Self>> {
        let buf = match ffs.read(SETTINGS_JSON) {
            Ok(buf) => buf,
            Err(err) if err.kind() == io::ErrorKind::NotFound =>
                return Ok(None),
            Err(err) =>
                return Err(err).context("Failed to read settings.json"),
        };
        let settings = Self::deserialize_json(&buf)?;
        Ok(Some(settings))
    }

    /// Merge updated settings from `update` into `self`.
    pub(crate) fn update(&mut self, update: Self) -> anyhow::Result<()> {
        ensure!(
            self.schema == update.schema,
            "Trying to update settings of a different schema version (persisted={}, update={}). \
             Somehow migrations didn't run?",
            self.schema.0,
            update.schema.0,
        );

        self.locale.update(update.locale);
        self.fiat_currency.update(update.fiat_currency);

        Ok(())
    }

    fn serialize_json(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .context("Failed to serialize settings.json")
    }

    fn deserialize_json(s: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(s).context("Failed to deserialize settings.json")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SchemaVersion::CURRENT,
            locale: None,
            fiat_currency: None,
        }
    }
}

// --- impl Version --- //

impl SchemaVersion {
    /// The current settings schema version.
    pub(crate) const CURRENT: Self = Self(1);
}

// --- Option<T>::update --- //

trait OptionExt {
    fn update(&mut self, update: Self);
}

impl<T> OptionExt for Option<T> {
    fn update(&mut self, update: Self) {
        if let Some(x) = update {
            *self = Some(x);
        }
    }
}

// #[cfg(test)]
// mod arb {
//     use proptest::{
//         arbitrary::Arbitrary,
//         strategy::{BoxedStrategy, Just, Strategy},
//     };
//
//     use super::*;
//
//     impl Arbitrary for SchemaVersion {
//         type Strategy = BoxedStrategy<Self>;
//         type Parameters = ();
//         fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
//             proptest::prop_oneof![
//                 10 => Just(Self::CURRENT),
//                 1 => (0_u32..10).prop_map(Self),
//             ]
//             .boxed()
//         }
//     }
// }

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use super::*;
    use crate::ffs::FlatFileFs;

    // struct ModelDb {
    //     ffs: ffs::test::MockFfs,
    //     settings: Settings,
    // }
    //
    // enum Op {
    //     Update(Settings),
    //     Read,
    //     Reload,
    // }

    #[tokio::test]
    async fn test_load_shutdown_load() {
        // logger::init_for_testing();

        let tmpdir = tempfile::TempDir::new().unwrap();
        let ffs = FlatFileFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        {
            let mut db = SettingsDb::load(ffs.clone());
            assert_eq!(
                db.settings.lock().unwrap().deref(),
                &Settings::default()
            );

            // update: locale=USD
            db.update(Settings {
                locale: Some("USD".to_owned()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.settings.lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    ..Default::default()
                }
            );

            // update: fiat_currency=USD
            db.update(Settings {
                fiat_currency: Some(IsoCurrencyCode::USD),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.settings.lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }

        {
            let mut db = SettingsDb::load(ffs.clone());
            assert_eq!(
                db.settings.lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    ..Default::default()
                }
            );
            db.shutdown().await.unwrap();
        }

        println!(
            "{}",
            String::from_utf8(ffs.read(SETTINGS_JSON).unwrap()).unwrap()
        );
    }
}
