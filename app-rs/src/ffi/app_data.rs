use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use flutter_rust_bridge::RustOpaqueNom;
use lexe_api::types::offer::LxOffer as LxOfferRs;

use crate::{
    app_data::{AppDataRs, HumanAddressRs},
    db::WritebackDb as WritebackDbRs,
    ffi::{
        api::HumanAddress,
        types::{Offer, Username},
    },
};

pub struct AppDataDb {
    pub inner: RustOpaqueNom<WritebackDbRs<AppDataRs>>,
}

pub struct AppData {
    pub human_address: Option<HumanAddress>,
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
        AppData::try_from(self.inner.read()).context("Failed to read AppData")
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
        let update_rs = AppDataRs::try_from(update)
            .context("Dart settings update is invalid")?;
        self.inner
            .update(update_rs)
            .context("Failed to apply settings update")?;
        Ok(())
    }
}

// --- impl AppData --- //

impl TryFrom<AppDataRs> for AppData {
    type Error = anyhow::Error;

    fn try_from(a: AppDataRs) -> Result<Self, Self::Error> {
        let human_address =
            a.human_address.map(HumanAddress::try_from).transpose()?;

        Ok(Self { human_address })
    }
}

impl TryFrom<HumanAddressRs> for HumanAddress {
    type Error = anyhow::Error;
    fn try_from(a: HumanAddressRs) -> Result<Self, Self::Error> {
        let username = a
            .username
            .map(|u| Username::parse(u.as_str()))
            .transpose()?;
        let offer = a
            .offer
            .map(|o| LxOfferRs::from_str(o.as_str()))
            .transpose()?
            .map(Offer::from);
        let updated_at = a.updated_at;
        let updatable = a.updatable;

        Ok(Self {
            username,
            offer,
            updated_at,
            updatable,
        })
    }
}

impl TryFrom<AppData> for AppDataRs {
    type Error = anyhow::Error;
    fn try_from(a: AppData) -> Result<Self, Self::Error> {
        let human_address =
            a.human_address.map(HumanAddressRs::try_from).transpose()?;

        Ok(Self {
            schema: AppDataRs::CURRENT_SCHEMA,
            human_address,
        })
    }
}
impl TryFrom<HumanAddress> for HumanAddressRs {
    type Error = anyhow::Error;
    fn try_from(a: HumanAddress) -> Result<Self, Self::Error> {
        let username = a.username.map(|u| u.into_inner());
        let offer = a.offer.map(|o| o.string);
        let updated_at = a.updated_at;
        let updatable = a.updatable;

        Ok(Self {
            username,
            offer,
            updated_at,
            updatable,
        })
    }
}
