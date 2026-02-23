//! App state database.

use anyhow::Context;
use lexe::ffs::Ffs;
#[cfg(doc)]
use lexe_api::models::command::HumanBitcoinAddress;
use lexe_api::types::{offer::LxOffer, username::Username};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::db::{SchemaVersion, Update, WritebackDb};

const APP_JSON: &str = "app.json";

/// In-memory app state.
// We don't use LxOffer and Username here because
// they are leaked into the FFI and we don't want
// to.
// TODO(maurice): Find out why we are leaking these types
// into the frb_generated.rs file.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct AppDataRs {
    /// AppDb schema version.
    pub schema: SchemaVersion,
    /// User's human Bitcoin address.
    // compat: alias added in app-v0.9.3
    #[serde(alias = "payment_address")]
    pub human_address: Option<HumanBitcoinAddressRs>,
}

impl AppDataRs {
    pub fn load<F: Ffs + Send + 'static>(ffs: F) -> WritebackDb<AppDataRs> {
        WritebackDb::<AppDataRs>::load(ffs, APP_JSON, "app")
    }

    pub(crate) const CURRENT_SCHEMA: SchemaVersion = SchemaVersion(1);
}

impl Update for AppDataRs {
    /// Merge updated settings from `update` into `self`.
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.schema
            .ensure_matches(update.schema)
            .context("AppDb schema version mismatch")?;
        self.human_address.update(update.human_address)?;
        Ok(())
    }
}

impl Default for AppDataRs {
    fn default() -> Self {
        Self {
            schema: AppDataRs::CURRENT_SCHEMA,
            human_address: None,
        }
    }
}

/// In-memory HBA state.
///
/// Serialized [`HumanBitcoinAddress`] struct that stores,
/// Usename and Offer as Strings and timestamps as i64 since
/// we don't want to leak the underlying types.
/// TODO(maurice): We should probably want to use the LxOffer and Username
/// types directly after fixing the leaks.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct HumanBitcoinAddressRs {
    /// User's username to receive BIP353 and lnurl payments.
    pub username: Option<String>,
    /// User's preferred Offer.
    pub offer: Option<String>,
    /// Last updated timestamp.
    pub updated_at: Option<i64>,
    /// Whether the user can update their HBA.
    pub updatable: bool,
}

impl Default for HumanBitcoinAddressRs {
    fn default() -> Self {
        Self {
            username: None,
            offer: None,
            updated_at: None,
            updatable: true,
        }
    }
}

impl Update for LxOffer {}
impl Update for Username {}
impl Update for i64 {}
impl Update for HumanBitcoinAddressRs {
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.username = update.username;
        self.offer = update.offer;
        self.updated_at = update.updated_at;
        self.updatable = update.updatable;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use lexe::ffs::DiskFs;

    use super::*;

    fn load_db(ffs: DiskFs) -> WritebackDb<AppDataRs> {
        WritebackDb::<AppDataRs>::load(ffs, APP_JSON, "test")
    }

    #[tokio::test]
    async fn test_load_shutdown_load() {
        // logger::init_for_testing();

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = DiskFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let dummy_offer = "lno1pgx9getnwss8vetrw3hhyuckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg".to_owned();
        let dummy_username = "dummy".to_owned();
        let dummy_updated_at = 1686743442000;
        {
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: None,
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }
        {
            let mut db = load_db(ffs.clone());
            assert_eq!(db.db().lock().unwrap().deref(), &AppDataRs::default());

            // update: offer=DummyOffer
            db.update(AppDataRs {
                human_address: Some(HumanBitcoinAddressRs {
                    offer: Some(dummy_offer.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        offer: Some(dummy_offer.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: username=dummy
            db.update(AppDataRs {
                human_address: Some(HumanBitcoinAddressRs {
                    username: Some(dummy_username.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        username: Some(dummy_username.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: updated_at=1686743442000
            db.update(AppDataRs {
                human_address: Some(HumanBitcoinAddressRs {
                    updated_at: Some(dummy_updated_at),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        updated_at: Some(dummy_updated_at),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: updatable=true
            db.update(AppDataRs {
                human_address: Some(HumanBitcoinAddressRs {
                    updatable: true,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        updatable: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: all fields
            db.update(AppDataRs {
                human_address: Some(HumanBitcoinAddressRs {
                    username: Some(dummy_username.clone()),
                    offer: Some(dummy_offer.clone()),
                    updated_at: Some(dummy_updated_at),
                    updatable: true,
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        username: Some(dummy_username.clone()),
                        offer: Some(dummy_offer.clone()),
                        updated_at: Some(dummy_updated_at),
                        updatable: true,
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
                &AppDataRs {
                    human_address: Some(HumanBitcoinAddressRs {
                        offer: Some(dummy_offer.clone()),
                        username: Some(dummy_username.clone()),
                        updated_at: Some(dummy_updated_at),
                        updatable: true,
                    }),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }
    }
}
