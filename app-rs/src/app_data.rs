//! App state database.

use anyhow::Context;
#[cfg(doc)]
use lexe_api::models::command::PaymentAddress;
use lexe_api::types::{offer::LxOffer, username::Username};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{
    db::{SchemaVersion, Update, WritebackDb},
    ffs::Ffs,
};

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
    /// User's PaymentAddress.
    pub payment_address: Option<PaymentAddressRs>,
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
        self.payment_address.update(update.payment_address)?;
        Ok(())
    }
}

impl Default for AppDataRs {
    fn default() -> Self {
        Self {
            schema: AppDataRs::CURRENT_SCHEMA,
            payment_address: None,
        }
    }
}

/// In-Memory PaymentAddress state.
///
/// Serialized [`PaymentAddress`] struct that stores,
/// Usename and Offer as Strings and timestamps as i64 since
/// we don't want to leak the underlying types.
/// TODO(maurice): We should probably want to use the LxOffer and Username
/// types directly after fixing the leaks.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct PaymentAddressRs {
    /// User's username to receive BIP353 and lnurl payments.
    pub username: Option<String>,
    /// User's preferred Offer.
    pub offer: Option<String>,
    /// Last updated timestamp.
    pub updated_at: Option<i64>,
    /// Whether the user can update their PaymentAddress.
    pub updatable: bool,
}

impl Default for PaymentAddressRs {
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
impl Update for PaymentAddressRs {
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.username.update(update.username)?;
        self.offer.update(update.offer)?;
        self.updated_at.update(update.updated_at)?;
        self.updatable.update(update.updatable)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use super::*;
    use crate::ffs::FlatFileFs;

    fn load_db(ffs: FlatFileFs) -> WritebackDb<AppDataRs> {
        WritebackDb::<AppDataRs>::load(ffs, APP_JSON, "test")
    }

    #[tokio::test]
    async fn test_load_shutdown_load() {
        // logger::init_for_testing();

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = FlatFileFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let dummy_offer = "lno1pgx9getnwss8vetrw3hhyuckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg".to_owned();
        let dummy_username = "dummy".to_owned();
        let dummy_updated_at = 1686743442000;
        {
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: None,
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
                payment_address: Some(PaymentAddressRs {
                    offer: Some(dummy_offer.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: Some(PaymentAddressRs {
                        offer: Some(dummy_offer.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: username=dummy
            db.update(AppDataRs {
                payment_address: Some(PaymentAddressRs {
                    username: Some(dummy_username.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: Some(PaymentAddressRs {
                        offer: Some(dummy_offer.clone()),
                        username: Some(dummy_username.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: updated_at=1686743442000
            db.update(AppDataRs {
                payment_address: Some(PaymentAddressRs {
                    updated_at: Some(dummy_updated_at),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: Some(PaymentAddressRs {
                        offer: Some(dummy_offer.clone()),
                        username: Some(dummy_username.clone()),
                        updated_at: Some(dummy_updated_at),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            );

            // update: updatable=true
            db.update(AppDataRs {
                payment_address: Some(PaymentAddressRs {
                    updatable: true,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: Some(PaymentAddressRs {
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

        {
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    payment_address: Some(PaymentAddressRs {
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
