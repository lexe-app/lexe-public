//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Used in `hex` module. Not super necessary, but convenient.
#![feature(slice_as_chunks)]
// Used in `rng` module. Avoids a runtime panic.
#![feature(const_option)]
// once_cell replacement in std (called LazyLock)
#![feature(lazy_cell)]
// Used in `enclave/sgx` module for sealing.
#![feature(split_array)]
// Allow e.g. warp::Filter<Extract: warp::Reply> syntax
#![feature(associated_type_bounds)]
// Allow adding side effects to Option and Result chains
#![feature(result_option_inspect)]
// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// Ignore this issue with `proptest_derive::Arbitrary`.
#![allow(clippy::arc_with_non_send_sync)]

// Some re-exports to prevent having to re-declare dependencies
pub use reqwest;
pub use secrecy::Secret;

/// API definitions, errors, clients, and structs sent across the wire.
pub mod api;
/// Remote attestation.
pub mod attest;
/// Exponential backoff.
pub mod backoff;
/// [`tokio::Bytes`](bytes::Bytes) but must contain a string.
pub mod byte_str;
/// User node CLI.
pub mod cli;
/// Mobile client to the node.
pub mod client;
/// Application-level constants.
pub mod constants;
/// Ed25519 types.
pub mod ed25519;
/// SGX types.
pub mod enclave;
/// Hex utils
pub mod hex;
/// serde_with helper for bytes types.
pub mod hexstr_or_bytes;
/// Iterator extensions.
pub mod iter;
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
/// A channel for sending deduplicated notifications with no data attached.
pub mod notify;
/// Random number generation.
pub mod rng;
/// `RootSeed`.
pub mod root_seed;
/// sha256 convenience module.
pub mod sha256;
/// `ShutdownChannel`.
pub mod shutdown;
/// `LxTask`.
pub mod task;
/// `TestEvent`.
pub mod test_event;
/// `TimestampMs`
pub mod time;
/// Encrypt/decrypt VFS blobs for remote storage.
pub mod vfs_encrypt;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

/// Assert at compile that that a boolean expression evaluates to true.
/// Implementation copied from the static_assertions crate.
#[macro_export]
macro_rules! const_assert {
    ($x:expr $(,)?) => {
        #[allow(unknown_lints, clippy::eq_op)]
        const _: [(); 0 - !{
            const CONST_ASSERT: bool = $x;
            CONST_ASSERT
        } as usize] = [];
    };
}

/// Assert at compile time that two `usize` values are equal. This assert has a
/// nice benefit where there compiler error will actually _print out_ the
/// two values.
#[macro_export]
macro_rules! const_assert_usize_eq {
    ($x:expr, $y:expr $(,)?) => {
        const _: [(); $x] = [(); $y];
    };
}

/// Compile-time cast from `&T::From` to `&T`, where `T` is just a struct with
/// a single field of type `T::From` and `T` is `#[repr(transparent)]`.
///
/// Useful for casting a new-type's inner struct reference to a new-type
/// reference.
///
/// See [`ref_cast`] for more details. Just use `T::ref_cast` if you don't need
/// `const`.
///
/// ## Example
///
/// ```rust
/// use common::const_ref_cast;
/// use ref_cast::RefCast;
///
/// #[derive(RefCast)]
/// #[repr(transparent)]
/// struct Id(u32);
///
/// // Safe, const cast from `&123` to `&Id(123)`
/// const MY_ID: &'static Id = const_ref_cast(&123);
/// ```
pub const fn const_ref_cast<T: ref_cast::RefCast>(from: &T::From) -> &T {
    // SAFETY: we require that `T: RefCast`, which guarantees that this cast is
    // safe. Unfortunately we need this extra method as `T::ref_cast` is not
    // currently const (Rust doesn't support const traits yet).
    unsafe { &*(from as *const T::From as *const T) }
}

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
