//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Used in `hex` module. Not super necessary, but convenient.
#![feature(slice_as_chunks)]
// Used in `rng` module. Avoids a runtime panic.
#![feature(const_option)]
// Used in `enclave/sgx` module for sealing.
#![feature(split_array)]
// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

use ref_cast::RefCast;
// re-export some common types from our dependencies
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
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
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
pub const fn const_ref_cast<T: RefCast>(from: &T::From) -> &T {
    // SAFETY: we require that `T: RefCast`, which guarantees that this cast is
    // safe. Unfortunately we need this extra method as `T::ref_cast` is not
    // currently const (Rust doesn't support const traits yet).
    unsafe { &*(from as *const T::From as *const T) }
}
