//! `common` contains types and functionality shared between most Lexe crates.

// Ignore this issue with `proptest_derive::Arbitrary`.
#![allow(clippy::arc_with_non_send_sync)]
// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]
// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

// Some re-exports to prevent having to re-declare dependencies
pub use byte_array::ByteArray;
pub use ref_cast::RefCast;
pub use secrecy::{ExposeSecret, Secret};

/// Encrypt/decrypt blobs for remote storage.
pub mod aes;
/// API definitions, errors, clients, and structs sent across the wire.
pub mod api;
/// [`tokio::Bytes`](bytes::Bytes) but must contain a string.
pub mod byte_str;
/// User node CLI.
pub mod cli;
/// Application-level constants.
pub mod constants;
/// [`dotenvy`] extensions.
pub mod dotenv;
/// Ed25519 types.
pub mod ed25519;
/// SGX types.
pub mod enclave;
/// `DeployEnv`.
pub mod env;
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
/// Networking utilities.
pub mod net;
/// `OrEnvExt` utility trait.
pub mod or_env;
/// Password-based encryption for arbitrary bytes.
pub mod password;
/// Random number generation.
pub mod rng;
/// `RootSeed`.
pub mod root_seed;
/// [`serde`] helpers.
pub mod serde_helpers;
/// `TimestampMs` and `DisplayMs`.
pub mod time;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

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
