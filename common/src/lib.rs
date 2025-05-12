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
pub use reqwest;
pub use secrecy::{ExposeSecret, Secret};

/// Encrypt/decrypt blobs for remote storage.
pub mod aes;
/// API definitions, errors, clients, and structs sent across the wire.
pub mod api;
/// `[u8; N]` array functions.
pub mod array;
/// Exponential backoff.
pub mod backoff;
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
/// Iterator extensions.
pub mod iter;
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
/// Networking utilities.
pub mod net;
/// A channel for sending deduplicated notifications with no data attached.
pub mod notify;
/// `NotifyOnce`.
pub mod notify_once;
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
/// `LxTask`.
pub mod task;
/// `TestEvent`.
pub mod test_event;
/// `TimestampMs` and `DisplayMs`.
pub mod time;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

/// A trait which allows us to apply functions (including tuple enum variants)
/// to non-[`Iterator`]/[`Result`]/[`Option`] values for cleaner iterator-like
/// chains. It exposes an [`apply`] method and is implemented for all `T`.
///
/// For example, instead of this:
///
/// ```
/// # use common::ln::amount::Amount;
/// let value_sat_u64 = 100_000u64; // Pretend this is from LDK
/// let value_sat_u32 = u32::try_from(value_sat_u64)
///     .expect("Amount shouldn't have overflowed");
/// let maybe_value = Some(Amount::from_sats_u32(value_sat_u32));
/// ```
///
/// We can remove the useless `value_sat_u32` intermediate variable:
///
/// ```
/// # use common::ln::amount::Amount;
/// # use common::Apply;
/// let value_sat_u64 = 100_000u64; // Pretend this is from LDK
/// let maybe_value = u32::try_from(value_sat_u64)
///     .expect("Amount shouldn't have overflowed")
///     .apply(Amount::from_sats_u32)
///     .apply(Some);
/// ```
///
/// Without having to add use nested [`Option`]s / [`Result`]s which can be
/// confusing:
///
/// ```
/// # use common::ln::amount::Amount;
/// let value_sat_u64 = 100_000u64; // Pretend this is from LDK
/// let maybe_value = u32::try_from(value_sat_u64)
///     .map(Amount::from_sats_u32)
///     .map(Some)
///     .expect("Amount shouldn't have overflowed");
/// ```
///
/// Overall, this trait makes it easier to both (1) write iterator chains
/// without unwanted intermediate variables and (2) write them in a way that
/// maximizes clarity and readability, instead of having to reorder our
/// `.transpose()` / `.map()`/ `.expect()` / `.context("...")?` operations
/// to "stay inside" the function chain.
///
/// [`apply`]: Self::apply
pub trait Apply<F, T> {
    fn apply(self, f: F) -> T;
}

impl<F, T, U> Apply<F, U> for T
where
    F: FnOnce(T) -> U,
{
    #[inline]
    fn apply(self, f: F) -> U {
        f(self)
    }
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
