//! App state database.

use anyhow::Context;
use lexe::{ffs::Ffs, types::command::GetHumanBitcoinAddressResponse};
use serde::{Deserialize, Deserializer, Serialize};

use crate::db::{SchemaVersion, Update, WritebackDb};

const APP_JSON: &str = "app.json";

/// The app's persisted database. Held in memory and written back to `app.json`
/// asynchronously by the [`WritebackDb`].
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct AppDataRs {
    /// AppDb schema version.
    pub schema: SchemaVersion,
    /// Best-effort display cache of the user's active Human Bitcoin Address.
    ///
    /// A malformed or legacy cached value (e.g. from before the HBA v2
    /// migration) deserializes to `None` rather than failing the whole load;
    /// the next `get_human_bitcoin_address` repopulates it.
    #[serde(default, deserialize_with = "deserialize_drop_invalid")]
    pub human_bitcoin_address: Option<GetHumanBitcoinAddressResponse>,
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
        self.human_bitcoin_address
            .update(update.human_bitcoin_address)?;
        Ok(())
    }
}

// The cached HBA is replaced wholesale, never field-merged.
impl Update for GetHumanBitcoinAddressResponse {}

impl Default for AppDataRs {
    fn default() -> Self {
        Self {
            schema: AppDataRs::CURRENT_SCHEMA,
            human_bitcoin_address: None,
        }
    }
}

/// Deserialize the cached HBA leniently: any malformed or legacy value degrades
/// to `None` instead of failing the whole `app.json` load. Relies on the
/// self-describing JSON format (the only format this db is (de)serialized as).
fn deserialize_drop_invalid<'de, D>(
    deserializer: D,
) -> Result<Option<GetHumanBitcoinAddressResponse>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

#[cfg(test)]
mod test {
    use std::{ops::Deref, str::FromStr};

    use lexe::ffs::DiskFs;
    use lexe_api::types::{offer::Offer, username::Username};

    use super::*;

    fn load_db(ffs: DiskFs) -> WritebackDb<AppDataRs> {
        WritebackDb::<AppDataRs>::load(ffs, APP_JSON, "test")
    }

    fn dummy_hba(username: &str) -> GetHumanBitcoinAddressResponse {
        let offer = Offer::from_str(
            "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q",
        )
        .unwrap();
        let username = Username::parse(username).unwrap();
        GetHumanBitcoinAddressResponse {
            human_bitcoin_address: username.human_bitcoin_address(),
            lightning_address: username.lightning_address(),
            offer,
            updatable: true,
        }
    }

    /// A cached HBA is replaced wholesale on update and survives a shutdown +
    /// reload.
    #[tokio::test]
    async fn test_load_shutdown_load() {
        let tmpdir = tempfile::tempdir().unwrap();
        let ffs = DiskFs::create_dir_all(tmpdir.path().to_owned()).unwrap();
        let final_hba = dummy_hba("second");

        {
            let mut db = load_db(ffs.clone());
            // Fresh db starts empty.
            assert_eq!(db.db().lock().unwrap().deref(), &AppDataRs::default());

            // Each update replaces the cached HBA wholesale.
            db.update(AppDataRs {
                human_bitcoin_address: Some(dummy_hba("first")),
                ..Default::default()
            })
            .unwrap();
            db.update(AppDataRs {
                human_bitcoin_address: Some(final_hba.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_bitcoin_address: Some(final_hba.clone()),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }

        {
            // The HBA persisted across the shutdown.
            let mut db = load_db(ffs.clone());
            assert_eq!(
                db.db().lock().unwrap().deref(),
                &AppDataRs {
                    human_bitcoin_address: Some(final_hba),
                    ..Default::default()
                }
            );

            db.shutdown().await.unwrap();
        }
    }

    /// A legacy or malformed cached HBA degrades to `None` rather than failing
    /// the whole `app.json` deserialization.
    #[test]
    fn invalid_cached_hba_drops_to_none() {
        // Legacy v1-shaped cache under the old `human_address` key (dropped in
        // the HBA v2 migration): the unknown key is ignored.
        let legacy = r#"{
            "schema": 1,
            "human_address": { "username": "alice", "updatable": true }
        }"#;
        let app_data: AppDataRs = serde_json::from_str(legacy)
            .expect("legacy app.json must still deserialize");
        assert!(app_data.human_bitcoin_address.is_none());

        // Legacy nested cache shape (from before the app cached the SDK's
        // flattened `GetHumanBitcoinAddressResponse`): dropped.
        let legacy_nested = r#"{
            "schema": 1,
            "human_bitcoin_address": {
                "hba": { "username": "alice" },
                "updatable": true
            }
        }"#;
        let app_data: AppDataRs = serde_json::from_str(legacy_nested)
            .expect("legacy nested cached HBA must not fail the load");
        assert!(app_data.human_bitcoin_address.is_none());

        // Malformed value under the current key: dropped, load still succeeds.
        let malformed = r#"{
            "schema": 1,
            "human_bitcoin_address": { "garbage": true }
        }"#;
        let app_data: AppDataRs = serde_json::from_str(malformed)
            .expect("malformed cached HBA must not fail the load");
        assert!(app_data.human_bitcoin_address.is_none());
    }
}
