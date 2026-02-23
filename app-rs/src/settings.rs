//! App settings db, serialization, and persistence.

use anyhow::Context;
use common::api::fiat_rates::IsoCurrencyCode;
use lexe::ffs::Ffs;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::db::{SchemaVersion, Update, WritebackDb};

const SETTINGS_JSON: &str = "settings.json";

/// In-memory app settings state.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct SettingsRs {
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

impl SettingsRs {
    pub fn load<F: Ffs + Send + 'static>(ffs: F) -> WritebackDb<SettingsRs> {
        WritebackDb::<SettingsRs>::load(ffs, SETTINGS_JSON, "settings")
    }

    /// The current settings schema version.
    pub(crate) const CURRENT_SCHEMA: SchemaVersion = SchemaVersion(1);
}

impl Update for SettingsRs {
    /// Merge updated settings from `update` into `self`.
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.schema
            .ensure_matches(update.schema)
            .context("Settings schema version mismatch")?;

        self.locale.update(update.locale)?;
        self.fiat_currency.update(update.fiat_currency)?;
        self.show_split_balances
            .update(update.show_split_balances)?;
        self.onboarding_status.update(update.onboarding_status)?;

        Ok(())
    }
}

impl Default for SettingsRs {
    fn default() -> Self {
        Self {
            schema: SettingsRs::CURRENT_SCHEMA,
            locale: None,
            fiat_currency: None,
            show_split_balances: None,
            onboarding_status: None,
        }
    }
}

/// Wallet funding state machine.
///
/// Tracks whether the user has funded their wallet.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub enum WalletFundingState {
    /// Initial state. User has no funds and no channel.
    NonFunded,
    /// User has received on-chain funds but has no Lightning channel yet.
    OnChainDeposited,
    /// User has a pending channel open. Waiting for confirmation.
    ChannelOpening,
    /// User has a usable Lightning channel but channel reserve is not met.
    /// Can receive but can't send.
    ChannelReserveNotMet,
    /// User has a usable Lightning channel with outbound capacity.
    Funded,
}

impl Default for WalletFundingState {
    fn default() -> Self {
        Self::NonFunded
    }
}

impl Update for WalletFundingState {}

/// In-Memory onboarding user state. Used to determine if we should ask
/// the user to finish their onboarding.
// TODO(maurice): Move this to the app_data module
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct OnboardingStatus {
    /// Whether the user confirmed they have backed up their seed phrase.
    pub has_backed_up_seed_phrase: Option<bool>,
    /// Whether the user has successfully connected their Google Drive.
    pub has_connected_gdrive: Option<bool>,
    /// Whether the user has seen the receive page carousel hint animation.
    pub has_seen_receive_hint: Option<bool>,
    /// The current wallet funding state.
    pub wallet_funding_state: Option<WalletFundingState>,
}

impl Update for OnboardingStatus {
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.has_backed_up_seed_phrase
            .update(update.has_backed_up_seed_phrase)?;
        self.has_connected_gdrive
            .update(update.has_connected_gdrive)?;
        self.has_seen_receive_hint
            .update(update.has_seen_receive_hint)?;
        self.wallet_funding_state
            .update(update.wallet_funding_state)?;
        Ok(())
    }
}

// --- impl Update --- //
impl Update for IsoCurrencyCode {}

#[cfg(test)]
mod test {
    use std::{ops::Deref, rc::Rc, time::Duration};

    use lexe::ffs::{DiskFs, test_utils::InMemoryFfs};
    use proptest::{proptest, strategy::Strategy};

    use super::*;
    use crate::db::{DbPersister, WritebackDb};

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
        let ffs = InMemoryFfs::new();
        ffs.write(SETTINGS_JSON, settings_str.as_bytes()).unwrap();
        let settings: SettingsRs = DbPersister::load(&ffs, SETTINGS_JSON);
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
        ffs: Rc<InMemoryFfs>,
        settings: SettingsRs,
    }

    impl ModelDb {
        fn load(ffs: Rc<InMemoryFfs>) -> Self {
            let settings = DbPersister::<InMemoryFfs, SettingsRs>::load(
                ffs.as_ref(),
                SETTINGS_JSON,
            );
            Self { ffs, settings }
        }
        fn read(&self) -> SettingsRs {
            self.settings.clone()
        }
        fn reset(&mut self) {
            self.settings = SettingsRs::default();
            let data = DbPersister::<InMemoryFfs, SettingsRs>::serialize_json(
                &self.settings,
            )
            .unwrap();
            self.ffs.write(SETTINGS_JSON, &data).unwrap();
        }
        fn update(&mut self, update: SettingsRs) -> anyhow::Result<()> {
            self.settings.update(update)?;
            let data = DbPersister::<InMemoryFfs, SettingsRs>::serialize_json(
                &self.settings,
            )?;
            self.ffs.write(SETTINGS_JSON, &data)?;
            Ok(())
        }
    }

    /// Operations we can perform against a [`SettingsDb`].
    #[derive(Debug, Arbitrary)]
    enum Op {
        Update(SettingsRs),
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

    fn load_db(ffs: DiskFs) -> WritebackDb<SettingsRs> {
        WritebackDb::<SettingsRs>::load(ffs, SETTINGS_JSON, "test")
    }

    async fn test_prop_model_inner(ops: Vec<Op>) {
        let model_ffs = Rc::new(InMemoryFfs::new());
        let mut model = ModelDb::load(model_ffs.clone());

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = DiskFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
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
        let ffs = DiskFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        {
            let mut db = load_db(ffs.clone());
            assert_eq!(db.db().lock().unwrap().deref(), &SettingsRs::default());

            // update: locale=USD
            db.update(SettingsRs {
                locale: Some("USD".to_owned()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    ..Default::default()
                }
            );

            // update: fiat_currency=USD
            db.update(SettingsRs {
                fiat_currency: Some(IsoCurrencyCode::USD),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    ..Default::default()
                }
            );

            // update: onboarding_status={ has_connected_gdrive: true }
            db.update(SettingsRs {
                onboarding_status: Some(OnboardingStatus {
                    has_backed_up_seed_phrase: None,
                    has_connected_gdrive: Some(true),
                    has_seen_receive_hint: None,
                    wallet_funding_state: None,
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_backed_up_seed_phrase: None,
                        has_connected_gdrive: Some(true),
                        has_seen_receive_hint: None,
                        wallet_funding_state: None,
                    }),
                    ..Default::default()
                }
            );

            // update: onboarding_status={ has_backed_up_seed_phrase: true }
            db.update(SettingsRs {
                onboarding_status: Some(OnboardingStatus {
                    has_backed_up_seed_phrase: Some(true),
                    has_connected_gdrive: None,
                    has_seen_receive_hint: None,
                    wallet_funding_state: None,
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_backed_up_seed_phrase: Some(true),
                        has_connected_gdrive: Some(true),
                        has_seen_receive_hint: None,
                        wallet_funding_state: None,
                    }),
                    ..Default::default()
                }
            );

            // update: onboarding_status={ has_seen_receive_hint: true }
            db.update(SettingsRs {
                onboarding_status: Some(OnboardingStatus {
                    has_backed_up_seed_phrase: None,
                    has_connected_gdrive: None,
                    has_seen_receive_hint: Some(true),
                    wallet_funding_state: None,
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_backed_up_seed_phrase: Some(true),
                        has_connected_gdrive: Some(true),
                        has_seen_receive_hint: Some(true),
                        wallet_funding_state: None,
                    }),
                    ..Default::default()
                }
            );

            // update: onboarding_status={ wallet_funding_state:
            // OnChainDeposited }
            db.update(SettingsRs {
                onboarding_status: Some(OnboardingStatus {
                    has_backed_up_seed_phrase: None,
                    has_connected_gdrive: None,
                    has_seen_receive_hint: None,
                    wallet_funding_state: Some(
                        WalletFundingState::OnChainDeposited,
                    ),
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_backed_up_seed_phrase: Some(true),
                        has_connected_gdrive: Some(true),
                        has_seen_receive_hint: Some(true),
                        wallet_funding_state: Some(
                            WalletFundingState::OnChainDeposited
                        ),
                    }),
                    ..Default::default()
                }
            );

            // update: onboarding_status={ wallet_funding_state: Funded }
            db.update(SettingsRs {
                onboarding_status: Some(OnboardingStatus {
                    has_backed_up_seed_phrase: None,
                    has_connected_gdrive: None,
                    has_seen_receive_hint: None,
                    wallet_funding_state: Some(WalletFundingState::Funded),
                }),
                ..Default::default()
            })
            .unwrap();

            assert_eq!(
                db.db().lock().unwrap().deref(),
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_connected_gdrive: Some(true),
                        has_backed_up_seed_phrase: Some(true),
                        has_seen_receive_hint: Some(true),
                        wallet_funding_state: Some(WalletFundingState::Funded),
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
                &SettingsRs {
                    locale: Some("USD".to_owned()),
                    fiat_currency: Some(IsoCurrencyCode::USD),
                    onboarding_status: Some(OnboardingStatus {
                        has_backed_up_seed_phrase: Some(true),
                        has_connected_gdrive: Some(true),
                        has_seen_receive_hint: Some(true),
                        wallet_funding_state: Some(WalletFundingState::Funded),
                    }),
                    ..Default::default()
                }
            );
            db.shutdown().await.unwrap();
        }
    }
}
