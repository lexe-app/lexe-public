//! Dart interface for app settings.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use common::api::fiat_rates::IsoCurrencyCode;
use flutter_rust_bridge::RustOpaqueNom;

use crate::{
    db::WritebackDb as WritebackDbRs,
    settings::{OnboardingStatus as OnboardingStatusRs, SettingsRs},
};

pub struct SettingsDb {
    pub inner: RustOpaqueNom<WritebackDbRs<SettingsRs>>,
}

pub struct OnboardingStatus {
    pub has_connected_gdrive: Option<bool>,
    pub has_backed_up_seed_phrase: Option<bool>,
}

pub struct Settings {
    pub locale: Option<String>,
    pub fiat_currency: Option<String>,
    pub show_split_balances: Option<bool>,
    pub onboarding_status: Option<OnboardingStatus>,
}

// --- impl SettingsDb --- //

impl SettingsDb {
    pub(crate) fn new(db: Arc<WritebackDbRs<SettingsRs>>) -> Self {
        Self {
            inner: RustOpaqueNom::from(db),
        }
    }

    /// Read all settings.
    ///
    /// flutter_rust_bridge:sync
    pub fn read(&self) -> Settings {
        Settings::from(self.inner.read())
    }

    /// Reset all settings to their defaults.
    ///
    /// flutter_rust_bridge:sync
    pub fn reset(&self) {
        self.inner.reset();
    }

    /// Update the in-memory settings by merging in any non-null fields in
    /// `update`. The settings will be persisted asynchronously, outside of this
    /// call.
    ///
    /// flutter_rust_bridge:sync
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
            show_split_balances: s.show_split_balances,
            onboarding_status: s.onboarding_status.map(OnboardingStatus::from),
        }
    }
}

impl From<OnboardingStatusRs> for OnboardingStatus {
    fn from(s: OnboardingStatusRs) -> Self {
        Self {
            has_connected_gdrive: s.has_connected_gdrive,
            has_backed_up_seed_phrase: s.has_backed_up_seed_phrase,
        }
    }
}

impl From<OnboardingStatus> for OnboardingStatusRs {
    fn from(s: OnboardingStatus) -> Self {
        Self {
            has_connected_gdrive: s.has_connected_gdrive,
            has_backed_up_seed_phrase: s.has_backed_up_seed_phrase,
        }
    }
}

impl TryFrom<Settings> for SettingsRs {
    type Error = anyhow::Error;
    fn try_from(s: Settings) -> Result<Self, Self::Error> {
        Ok(Self {
            schema: SettingsRs::CURRENT_SCHEMA,
            locale: s.locale,
            fiat_currency: s
                .fiat_currency
                .as_deref()
                .map(IsoCurrencyCode::from_str)
                .transpose()?,
            show_split_balances: s.show_split_balances,
            onboarding_status: s
                .onboarding_status
                .map(OnboardingStatusRs::from),
        })
    }
}
