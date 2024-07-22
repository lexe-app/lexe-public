//! Dart interface for app settings.

use std::str::FromStr;

use common::api::fiat_rates::IsoCurrencyCode;
use flutter_rust_bridge::frb;

use crate::settings::Settings as SettingsRs;

#[frb(dart_metadata=("freezed"))]
pub struct Settings {
    pub locale: Option<String>,
    pub fiat_currency: Option<String>,
}

pub fn save(settings: Settings) -> anyhow::Result<Settings> {
    Ok(settings)
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
            version: None,
            locale: s.locale,
            fiat_currency: s
                .fiat_currency
                .as_deref()
                .map(IsoCurrencyCode::from_str)
                .transpose()?,
        })
    }
}
