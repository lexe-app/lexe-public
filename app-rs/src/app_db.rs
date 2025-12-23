//! App state database.

#![allow(unused)]
#![allow(dead_code)]

use anyhow::Context;
use lexe_api::types::{offer::LxOffer, username::Username};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use sdk_rust::ffs::Ffs;

use crate::db::{SchemaVersion, Update, WritebackDb};

const APP_JSON: &str = "app.json";

/// In-memory app state.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub(crate) struct AppDb {
    /// AppDb schema version.
    pub schema: SchemaVersion,
    /// User's preferred Offer. Based on the user's PaymentAddress.
    pub offer: Option<LxOffer>,
    /// User's username to receive BIP353 and lnurl payments.
    pub username: Option<Username>,
}

impl AppDb {
    pub(crate) fn load<F: Ffs + Send + 'static>(ffs: F) -> WritebackDb<AppDb> {
        WritebackDb::<AppDb>::load(ffs, APP_JSON, "app")
    }

    pub(crate) const CURRENT_SCHEMA: SchemaVersion = SchemaVersion(1);
}

impl Update for AppDb {
    /// Merge updated settings from `update` into `self`.
    fn update(&mut self, update: Self) -> anyhow::Result<()> {
        self.schema
            .ensure_matches(update.schema)
            .context("AppDb schema version mismatch")?;
        self.offer.update(update.offer)?;
        self.username.update(update.username)?;
        Ok(())
    }
}

impl Default for AppDb {
    fn default() -> Self {
        Self {
            schema: AppDb::CURRENT_SCHEMA,
            offer: None,
            username: None,
        }
    }
}

impl Update for LxOffer {}
impl Update for Username {}

#[cfg(test)]
mod test {
    use std::{ops::Deref, str::FromStr};

    use super::*;
    use sdk_rust::ffs::DiskFs;

    fn load_db(ffs: DiskFs) -> WritebackDb<AppDb> {
        WritebackDb::<AppDb>::load(ffs, APP_JSON, "test")
    }

    #[tokio::test]
    async fn test_load_shutdown_load() {
        // logger::init_for_testing();

        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = DiskFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let dummy_offer = LxOffer::from_str(
                "lno1pgx9getnwss8vetrw3hhyuckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg",
            ).unwrap();
        let dummy_username = Username::parse("dummy").unwrap();
        {
            let mut db = load_db(ffs.clone());
            assert_eq!(db.db().lock().unwrap().deref(), &AppDb::default());

            // update: offer=DummyOffer
            db.update(AppDb {
                offer: Some(dummy_offer.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDb {
                    offer: Some(dummy_offer.clone()),
                    ..Default::default()
                }
            );

            // update: username=dummy
            db.update(AppDb {
                username: Some(dummy_username.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDb {
                    offer: Some(dummy_offer.clone()),
                    username: Some(dummy_username.clone()),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }

        {
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDb {
                    offer: Some(dummy_offer),
                    username: Some(dummy_username),
                    ..Default::default()
                }
            );
            db.shutdown().await.unwrap();
        }
    }
}
