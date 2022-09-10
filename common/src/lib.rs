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

// re-export some common types from our dependencies
pub use bitcoin::secp256k1::PublicKey;
use ref_cast::RefCast;
pub use secrecy::Secret;

pub mod api;
pub mod attest;
pub mod auth;
pub mod cli;
pub mod client;
pub mod constants;
pub mod ed25519;
pub mod enclave;
pub mod hex;
pub mod hexstr_or_bytes;
pub mod ln;
pub mod rng;
pub mod root_seed;
pub mod sha256;
pub mod shutdown;
pub mod task;

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
