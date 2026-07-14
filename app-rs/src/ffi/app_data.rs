use std::sync::Arc;

use anyhow::Context;
use flutter_rust_bridge::RustOpaqueNom;
use lexe::types::command::GetHumanBitcoinAddressResponse as GetHumanBitcoinAddressResponseRs;

use crate::{
    app_data::AppDataRs, db::WritebackDb as WritebackDbRs,
    ffi::api::GetHumanBitcoinAddressResponse,
};

pub struct AppDataDb {
    pub inner: RustOpaqueNom<WritebackDbRs<AppDataRs>>,
}

pub struct AppData {
    pub human_bitcoin_address: Option<GetHumanBitcoinAddressResponse>,
}

//  --- impl AppDataDb --- //

impl AppDataDb {
    pub(crate) fn new(db: Arc<WritebackDbRs<AppDataRs>>) -> Self {
        Self {
            inner: RustOpaqueNom::from(db),
        }
    }

    /// Read all data from the db.
    ///
    /// flutter_rust_bridge:sync
    pub fn read(&self) -> anyhow::Result<AppData> {
        Ok(AppData::from(self.inner.read()))
    }

    /// Reset all data to their defaults.
    ///
    /// flutter_rust_bridge:sync
    pub fn reset(&self) {
        self.inner.reset();
    }

    /// Update the in-memory db by merging in any non-null fields in
    /// `update`. The db will be persisted asynchronously, outside of this
    /// call.
    ///
    /// flutter_rust_bridge:sync
    pub fn update(&self, update: AppData) -> anyhow::Result<()> {
        self.inner
            .update(AppDataRs::try_from(update)?)
            .context("Failed to apply settings update")
    }
}

// --- impl AppData --- //

impl From<AppDataRs> for AppData {
    fn from(a: AppDataRs) -> Self {
        Self {
            human_bitcoin_address: a
                .human_bitcoin_address
                .map(GetHumanBitcoinAddressResponse::from),
        }
    }
}

impl TryFrom<AppData> for AppDataRs {
    type Error = anyhow::Error;

    fn try_from(a: AppData) -> anyhow::Result<Self> {
        let human_bitcoin_address = a
            .human_bitcoin_address
            .map(GetHumanBitcoinAddressResponseRs::try_from)
            .transpose()
            .context("Invalid cached HBA")?;
        Ok(Self {
            schema: AppDataRs::CURRENT_SCHEMA,
            human_bitcoin_address,
        })
    }
}
