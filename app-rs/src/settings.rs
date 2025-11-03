//! App settings db, serialization, and persistence.

use anyhow::ensure;
use common::api::fiat_rates::IsoCurrencyCode;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{
    db::{SchemaVersion, Update, WritebackDb},
    ffs::Ffs,
};

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
    /// Onboarding state.
    pub onboarding_status: Option<OnboardingStatus>,
}

impl Settings {
    pub fn load<F: Ffs + Send + 'static>(ffs: F) -> WritebackDb<Settings> {
        WritebackDb::<Settings>::load(ffs, SETTINGS_JSON, "settings")
    }

    /// The current settings schema version.
    pub(crate) const CURRENT_SCHEMA: SchemaVersion = SchemaVersion(1);
}

impl Update for Settings {
    /// Merge updated settings from `update` into `self`.
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        ensure!(
            self.schema == update.schema,
            "Trying to update settings of a different schema version (persisted={}, update={}). \
             Somehow migrations didn't run?",
            self.schema.0,
            update.schema.0,
        );

        self.locale.update(update.locale)?;
        self.fiat_currency.update(update.fiat_currency)?;
        self.show_split_balances
            .update(update.show_split_balances)?;
        self.onboarding_status.update(update.onboarding_status)?;

        Ok(())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: Settings::CURRENT_SCHEMA,
            locale: None,
            fiat_currency: None,
            show_split_balances: None,
            onboarding_status: None,
        }
    }
}

/// In-Memory onboarding user state. Used to determine if we should ask
/// the user to finish their onboarding.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct OnboardingStatus {
    /// Whether the user has successfully connected ther Google Drive.
    pub has_connected_gdrive: Option<bool>,
    /// Whether the user confirmed they have backed up their seed phrase.
    pub has_backed_up_seed_phrase: Option<bool>,
}

impl Update for OnboardingStatus {
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.has_connected_gdrive
            .update(update.has_connected_gdrive)?;
        self.has_backed_up_seed_phrase
            .update(update.has_backed_up_seed_phrase)?;
        Ok(())
    }
}

// --- impl Update --- //
impl Update for IsoCurrencyCode {}

#[cfg(test)]
mod test {
    use std::{ops::Deref, rc::Rc, time::Duration};

    use proptest::{proptest, strategy::Strategy};

    use super::*;
    use crate::{
        db::{DbPersister, WritebackDb},
        ffs::{FlatFileFs, test::MockFfs},
    };

    #[test]
    fn test_load_hardcoded() {
        let settings_str = r#"
        {
            "schema": 1,
            "fiat_currency": "USD",
            "show_split_balances": true,
            "onboarding_status": {
                "has_connected_gdrive": true,
                "has_backed_up_seed_phrase": false
            }
        }
        "#;
        let ffs = MockFfs::new();
        ffs.write(SETTINGS_JSON, settings_str.as_bytes()).unwrap();
        let settings: Settings = DbPersister::load(&ffs, SETTINGS_JSON);
        assert_eq!(settings.schema, SchemaVersion(1));
        assert_eq!(settings.locale, None);
        assert_eq!(settings.fiat_currency, Some(IsoCurrencyCode::USD));
        assert_eq!(settings.show_split_balances, Some(true));
        let onboarding_status = settings.onboarding_status.unwrap();
        assert_eq!(onboarding_status.has_connected_gdrive, Some(true));
        assert_eq!(onboarding_status.has_backed_up_seed_phrase, Some(false));
    }

    /// A tiny model implementation of [`SettingsDb`].
    struct ModelDb {
        ffs: Rc<MockFfs>,
        settings: Settings,
    }

    impl ModelDb {
        fn load(ffs: Rc<MockFfs>) -> Self {
            let settings = DbPersister::<MockFfs, Settings>::load(
                ffs.as_ref(),
                SETTINGS_JSON,
            );
            Self { ffs, settings }
        }
        fn read(&self) -> Settings {
            self.settings.clone()
        }
        fn reset(&mut self) {
            self.settings = Settings::default();
            let data = DbPersister::<MockFfs, Settings>::serialize_json(
                &self.settings,
            )
            .unwrap();
            self.ffs.write(SETTINGS_JSON, &data).unwrap();
        }
        fn update(&mut self, update: Settings) -> anyhow::Result<()> {
            self.settings.update(update)?;
            let data = DbPersister::<MockFfs, Settings>::serialize_json(
                &self.settings,
            )?;
            self.ffs.write(SETTINGS_JSON, &data)?;
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

    fn load_db(ffs: FlatFileFs) -> WritebackDb<Settings> {
        WritebackDb::<Settings>::load(ffs, SETTINGS_JSON, "test")
    }

    async fn test_prop_model_inner(ops: Vec<Op>) {
        let model_ffs = Rc::new(MockFfs::new());
        let mut model = ModelDb::load(model_ffs.clone());

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = FlatFileFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let mut real = load_db(ffs.clone());

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
                    real = load_db(ffs.clone());
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
            let mut db = load_db(ffs.clone());
            assert_eq!(db.db().lock().unwrap().deref(), &Settings::default());

            // update: locale=USD
            db.update(Settings {
                locale: Some("USD".to_owned()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
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
                db.db().lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    ..Default::default()
                }
            );

            // update: onbarding_status={ has_connected_gdrive: true }
            db.update(Settings {
                onboarding_status: Some(OnboardingStatus {
                    has_connected_gdrive: Some(true),
                    has_backed_up_seed_phrase: None,
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_connected_gdrive: Some(true),
                        has_backed_up_seed_phrase: None,
                    }),
                    ..Default::default()
                }
            );

            // update: onbarding_status={ has_connected_gdrive: true }
            db.update(Settings {
                onboarding_status: Some(OnboardingStatus {
                    has_connected_gdrive: None,
                    has_backed_up_seed_phrase: Some(true),
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_connected_gdrive: Some(true),
                        has_backed_up_seed_phrase: Some(true),
                    }),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }

        {
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &Settings {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_connected_gdrive: Some(true),
                        has_backed_up_seed_phrase: Some(true),
                    }),
                    ..Default::default()
                }
            );
            db.shutdown().await.unwrap();
        }
    }
}
