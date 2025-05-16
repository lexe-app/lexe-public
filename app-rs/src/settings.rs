//! App settings db, serialization, and persistence.

use std::{io, sync::Arc, time::Duration};

use anyhow::{ensure, Context};
use common::{api::fiat_rates::IsoCurrencyCode, debug_panic_release_log};
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::ffs::Ffs;

const SETTINGS_JSON: &str = "settings.json";

/// In-memory app settings state.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct Settings {
    /// Settings schema version.
    pub schema: SchemaVersion,
    /// Preferred locale.
    pub locale: Option<String>,
    /// Perferred fiat currency (e.g. "USD").
    pub fiat_currency: Option<IsoCurrencyCode>,
    /// Show lightning and bitcoin sub-balances on the wallet home-page.
    pub show_split_balances: Option<bool>,
}

impl Settings {
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
        self.show_split_balances.update(update.show_split_balances);

        Ok(())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SchemaVersion::CURRENT,
            locale: None,
            fiat_currency: None,
            show_split_balances: None,
        }
    }
}

/// The app settings DB. Responsible for managing access to the settings.
///
/// Persistence is currently done asynchronously out-of-band, so calling
/// [`SettingsDb::update`] only modifies the in-memory state. The
/// [`SettingsPersister`] will finish writing the settings to durable storage
/// later (by at most 500ms).
pub(crate) struct SettingsDb {
    /// The current in-memory settings.
    settings: Arc<std::sync::Mutex<Settings>>,
    /// Notify the [`SettingsPersister`] to persist the settings.
    persist_tx: notify::Sender,
    /// Handle to spawned [`SettingsPersister`].
    persist_task: Option<LxTask<()>>,
    /// Trigger shutdown of [`SettingsPersister`].
    shutdown: NotifyOnce,
}

/// Persists settings asynchronously when notified by the [`SettingsDb`].
struct SettingsPersister<F> {
    /// Settings flat file store.
    ffs: F,
    /// The current in-memory settings.
    settings: Arc<std::sync::Mutex<Settings>>,
    /// Receives notifications when the settings have updated.
    persist_rx: notify::Receiver,
    /// Receives shutdown signal.
    shutdown: NotifyOnce,
}

/// Settings schema version. Used to determine whether to run migrations.
#[derive(Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(transparent)]
pub(crate) struct SchemaVersion(pub u32);

// --- impl SettingsDb --- //

impl SettingsDb {
    pub(crate) fn load<F: Ffs + Send + 'static>(ffs: F) -> Self {
        let settings = Arc::new(std::sync::Mutex::new(Settings::load(&ffs)));
        let (persist_tx, persist_rx) = notify::channel();
        let shutdown = NotifyOnce::new();

        // spawn a task that we can notify to write settings updates to durable
        // storage.
        let persister = SettingsPersister::new(
            ffs,
            settings.clone(),
            persist_rx,
            shutdown.clone(),
        );
        let persist_task =
            Some(LxTask::spawn("settings_persist", persister.run()));

        Self {
            settings,
            persist_tx,
            persist_task,
            shutdown,
        }
    }

    /// Shutdown the [`SettingsDb`]. Flushes any pending writes to disk.
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
            .context("settings persister failed to shutdown in time")?
            .context("settings persister panicked")
    }

    /// Return a clone of the current in-memory [`Settings`].
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn read(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Reset the in-memory [`Settings`] to its default value and notify the
    /// [`SettingsPersister`].
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn reset(&self) {
        *self.settings.lock().unwrap() = Settings::default();
        self.persist_tx.send();
    }

    /// Update the in-memory [`Settings`] by merging in any `Some` fields in
    /// `update`. Then notify the [`SettingsPersister`] that we need to save,
    /// but don't wait for it to actually persist.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
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
        shutdown: NotifyOnce,
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
            debug_panic_release_log!("Error persisting settings: {err:#}");
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
                debug_panic_release_log!("settings: failed to load: {err:#}");
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

    fn serialize_json(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .context("Failed to serialize settings.json")
    }

    fn deserialize_json(s: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(s).context("Failed to deserialize settings.json")
    }
}

// --- impl Version --- //

impl SchemaVersion {
    /// The current settings schema version.
    pub(crate) const CURRENT: Self = Self(1);
}

// --- Option<T>::update --- //

trait OptionExt {
    /// Replace `self` only if `update` is `Some(_)`.
    fn update(&mut self, update: Self);
}

impl<T> OptionExt for Option<T> {
    fn update(&mut self, update: Self) {
        if let Some(x) = update {
            *self = Some(x);
        }
    }
}

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
                10 => Just(Self::CURRENT),
                1 => (0_u32..10).prop_map(Self),
            ]
            .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use std::{ops::Deref, rc::Rc};

    use proptest::{proptest, strategy::Strategy};

    use super::*;
    use crate::ffs::{test::MockFfs, FlatFileFs};

    #[test]
    fn test_load_hardcoded() {
        let settings_str = r#"
        {
            "schema": 1,
            "fiat_currency": "USD",
            "show_split_balances": true
        }
        "#;
        let ffs = MockFfs::new();
        ffs.write(SETTINGS_JSON, settings_str.as_bytes()).unwrap();
        let settings = Settings::load(&ffs);
        assert_eq!(settings.schema, SchemaVersion(1));
        assert_eq!(settings.locale, None);
        assert_eq!(settings.fiat_currency, Some(IsoCurrencyCode::USD));
        assert_eq!(settings.show_split_balances, Some(true));
    }

    /// A tiny model implementation of [`SettingsDb`].
    struct ModelDb {
        ffs: Rc<MockFfs>,
        settings: Settings,
    }

    impl ModelDb {
        fn load(ffs: Rc<MockFfs>) -> Self {
            let settings = Settings::load(ffs.as_ref());
            Self { ffs, settings }
        }
        fn read(&self) -> Settings {
            self.settings.clone()
        }
        fn reset(&mut self) {
            self.settings = Settings::default();
            self.ffs
                .write(SETTINGS_JSON, &self.settings.serialize_json().unwrap())
                .unwrap();
        }
        fn update(&mut self, update: Settings) -> anyhow::Result<()> {
            self.settings.update(update)?;
            self.ffs
                .write(SETTINGS_JSON, &self.settings.serialize_json()?)?;
            Ok(())
        }
    }

    /// Operations we can perform against a [`SettingsDb`].
    #[derive(Debug, Arbitrary)]
    enum Op {
        Update(Settings),
        Read,
        Reset,
        Reload,

        /// Give background settings persister task time to do stuff. We use
        /// this in a fake-time Runtime, so it doesn't consume realtime.
        #[proptest(strategy = "Op::arb_short_sleep()")]
        Sleep(Duration),
    }

    impl Op {
        fn arb_short_sleep() -> impl Strategy<Value = Self> {
            (0_u64..=10)
                .prop_map(|x| Self::Sleep(Duration::from_millis(x * 100)))
        }
    }

    // Proptest
    #[test]
    fn test_prop_model() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            // Use fake time
            .start_paused(true)
            .build()
            .unwrap();

        let config = proptest::test_runner::Config::with_cases(50);
        proptest!(config, |(ops: Vec<Op>)| {
            rt.block_on(test_prop_model_inner(ops));
        });
    }

    async fn test_prop_model_inner(ops: Vec<Op>) {
        let model_ffs = Rc::new(MockFfs::new());
        let mut model = ModelDb::load(model_ffs.clone());

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = FlatFileFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let mut real = SettingsDb::load(ffs.clone());

        for op in ops {
            match op {
                Op::Update(update) => {
                    let res1 = model.update(update.clone());
                    let res2 = real.update(update);
                    assert_eq!(res1.is_ok(), res2.is_ok());
                }
                Op::Read => {
                    let s1 = model.read();
                    let s2 = real.read();
                    assert_eq!(s1, s2);
                }
                Op::Reset => {
                    model.reset();
                    real.reset();
                }
                Op::Reload => {
                    model = ModelDb::load(model_ffs.clone());

                    real.shutdown().await.unwrap();
                    real = SettingsDb::load(ffs.clone());
                }
                Op::Sleep(duration) => {
                    tokio::time::sleep(duration).await;
                }
            }
        }

        real.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_load_shutdown_load() {
        // logger::init_for_testing();

        let tmpdir = tempfile::tempdir().unwrap();
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
    }
}
