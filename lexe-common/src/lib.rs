//! `lexe-common` contains types and functionality shared between most Lexe
//! crates.

// Ignore this issue with `proptest_derive::Arbitrary`.
#![allow(clippy::arc_with_non_send_sync)]
// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]
// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

use std::path::PathBuf;

use anyhow::anyhow;
// Some re-exports to prevent having to re-declare dependencies
pub use lexe_byte_array::ByteArray;
// TODO(phlip9): remove re-exports
pub use lexe_crypto::{aes, ed25519, password};
pub use ref_cast::RefCast;
pub use secrecy::{ExposeSecret, Secret};

/// API definitions, errors, clients, and structs sent across the wire.
pub mod api;
/// [`tokio::Bytes`](bytes::Bytes) but must contain a string.
pub mod byte_str;
/// Application-level constants.
pub mod constants;
/// [`dotenvy`] extensions.
pub mod dotenv;
/// `DeployEnv`.
pub mod env;
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
/// Networking utilities.
pub mod net;
/// `OrEnvExt` utility trait.
pub mod or_env;
/// Types related to `releases.json`.
pub mod releases;
/// Random number generation.
pub mod rng;
/// `RootSeed`.
pub mod root_seed;
/// Global `Secp256k1` context
pub mod secp256k1_ctx;
/// `TimestampMs` and `DisplayMs`.
pub mod time;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

/// Returns the default Lexe data directory (`~/.lexe`).
pub fn default_lexe_data_dir() -> anyhow::Result<PathBuf> {
    #[allow(deprecated)] // home_dir is fine for our use case
    let home = std::env::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?;
    Ok(home.join(".lexe"))
}

/// `panic!(..)`s in debug mode, `tracing::error!(..)`s in release mode
#[macro_export]
macro_rules! debug_panic_release_log {
    ($($arg:tt)*) => {
        if core::cfg!(debug_assertions) {
            core::panic!($($arg)*);
        } else {
            tracing::error!($($arg)*);
        }
    };
}
