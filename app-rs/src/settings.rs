//! App settings db, serialization, and persistence.

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
    pub version: Option<u32>,

    /// Preferred locale.
    pub locale: Option<String>,

    /// Perferred fiat currency (e.g. "USD").
    pub fiat_currency: Option<IsoCurrencyCode>,
}

// Requirements:
//
// Option 1: Dart = source-of-truth, Rust = write-back cache + persistence
//
// Option 2: Dart = convenient UI, Rust = source-of-truth + persistence
//
// - most correct
// - make conversion sync but just enqueue update to be written?
//
// Like `shared_preferences`, data
//
// ```dart
// await settings.set(
//   locale: "USD",
// );
//
// settings.locale.value
//
// // idea: internally a ValueNotifier<T>, but exposed only as a ValueListenable<T>?
// settings.locale // : ValueNotifier<String?>
// ```
//
// ```rust
// tokio::sync::Mutex<SettingsDb>
// ```
//
// write:
//
// 1. dart: settings.set(locale: "USD")
// 2. rust: sync convert dart Settings -> rust Settings
//          + update in-memory rust Settings
//          + convert and return new rust Settings -> dart Settings
// 3. dart: update in-memory Settings
// 4. dart: each settings ValueListenable updates listeners
//
// ...
//
//    rust: (later) settings persister wakes up and serializes + persists
//    settings.json
//
// read:
//
// 1. dart: settings.locale.value
