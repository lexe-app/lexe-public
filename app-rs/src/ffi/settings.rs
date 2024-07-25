//! Dart interface for app settings.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use common::api::fiat_rates::IsoCurrencyCode;
use flutter_rust_bridge::{frb, RustOpaqueNom};

pub(crate) use crate::settings::SettingsDb as SettingsDbRs;
use crate::settings::{SchemaVersion, Settings as SettingsRs};

pub struct SettingsDb {
    pub inner: RustOpaqueNom<SettingsDbRs>,
}

pub struct Settings {
    pub locale: Option<String>,
    pub fiat_currency: Option<String>,
}

// --- impl SettingsDb --- //

impl SettingsDb {
    pub(crate) fn new(db: Arc<SettingsDbRs>) -> Self {
        Self {
            inner: RustOpaqueNom::from(db),
        }
    }

    #[frb(sync)]
    pub fn read(&self) -> Settings {
        Settings::from(self.inner.read())
    }

    #[frb(sync)]
    pub fn update(&self, update: Settings) -> anyhow::Result<()> {
        let update_rs = SettingsRs::try_from(update)
            .context("Dart settings update is invalid")?;
        self.inner
            .update(update_rs)
            .context("Failed to apply settings update")?;
        Ok(())
    }
}

// --- impl Settings --- //

impl From<SettingsRs> for Settings {
    fn from(s: SettingsRs) -> Self {
        Self {
            locale: s.locale,
            fiat_currency: s.fiat_currency.map(|x| x.as_str().to_owned()),
        }
    }
}

impl TryFrom<Settings> for SettingsRs {
    type Error = anyhow::Error;
    fn try_from(s: Settings) -> Result<Self, Self::Error> {
        Ok(Self {
            schema: SchemaVersion::CURRENT,
            locale: s.locale,
            fiat_currency: s
                .fiat_currency
                .as_deref()
                .map(IsoCurrencyCode::from_str)
                .transpose()?,
        })
    }
}
