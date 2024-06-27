//! Low-level app settings serialization and persistence.

use common::api::fiat_rates::IsoCurrencyCode;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub(crate) struct SettingsDb<F> {
    ffs: F,
    settings: Settings,
}

#[derive(Clone, Debug, Default, PartialEq)]
#[derive(Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// Preferred locale.
    pub locale: Option<String>,

    /// Perferred fiat currency (e.g. "USD").
    pub fiat_currency: Option<IsoCurrencyCode>,
}

// ```dart
// await settings.set(
//   locale: "USD",
// );
//
// settings.locale.value
//
// settings.locale // : ValueNotifier<String?>
// ```
//
//
// ```rust
// tokio::sync::Mutex<SettingsDb>
// ```
