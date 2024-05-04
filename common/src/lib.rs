//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Ignore this issue with `proptest_derive::Arbitrary`.
#![allow(clippy::arc_with_non_send_sync)]
// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]

// Some re-exports to prevent having to re-declare dependencies
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
/// Mobile client to the node.
pub mod client;
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
/// Hex utils
pub mod hex;
/// serde_with helper for bytes types.
pub mod hexstr_or_bytes;
/// `hex_str_or_bytes` but for [`Option`] bytes types.
pub mod hexstr_or_bytes_opt;
/// Iterator extensions.
pub mod iter;
/// Bitcoin / Lightning Lexe newtypes which can't go in lexe-ln
pub mod ln;
/// Networking utilities.
pub mod net;
/// A channel for sending deduplicated notifications with no data attached.
pub mod notify;
/// Password-based encryption for arbitrary bytes.
pub mod password;
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
/// TLS certs and configurations.
pub mod tls;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

/// Assert at compile that that a boolean expression evaluates to true.
/// Implementation copied from the static_assertions crate.
#[macro_export]
macro_rules! const_assert {
    ($x:expr $(,)?) => {
        #[allow(clippy::const_is_empty, clippy::eq_op, unknown_lints)]
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

/// [`Option::unwrap`] but works in `const fn`.
// TODO(phlip9): remove this when const unwrap stabilizes
pub const fn const_option_unwrap<T: Copy>(option: Option<T>) -> T {
    match option {
        Some(value) => value,
        None => panic!("unwrap on None"),
    }
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

/// Copies of nightly-only functions for `&[u8]`.
// TODO(phlip9): remove functions as they stabilize.
trait SliceExt {
    //
    // `<&[u8]>::as_chunks`
    //

    /// Splits the slice into a slice of `N`-element arrays,
    /// starting at the beginning of the slice,
    /// and a remainder slice with length strictly less than `N`.
    fn as_chunks_stable<const N: usize>(&self) -> (&[[u8; N]], &[u8]);

    unsafe fn as_chunks_unchecked_stable<const N: usize>(&self) -> &[[u8; N]];
}

impl SliceExt for [u8] {
    //
    // `<&[u8]>::as_chunks`
    //

    #[inline]
    fn as_chunks_stable<const N: usize>(&self) -> (&[[u8; N]], &[u8]) {
        assert!(N != 0, "chunk size must be non-zero");

        let len = self.len() / N;
        let (multiple_of_n, remainder) = self.split_at(len * N);
        // SAFETY: We already panicked for zero, and ensured by construction
        // that the length of the subslice is a multiple of N.
        let array_slice = unsafe { multiple_of_n.as_chunks_unchecked_stable() };
        (array_slice, remainder)
    }

    #[inline]
    unsafe fn as_chunks_unchecked_stable<const N: usize>(&self) -> &[[u8; N]] {
        // SAFETY: Caller must guarantee that `N` is nonzero and exactly divides
        // the slice length
        let new_len = self.len() / N;
        // SAFETY: We cast a slice of `new_len * N` elements into
        // a slice of `new_len` many `N` elements chunks.
        unsafe { std::slice::from_raw_parts(self.as_ptr().cast(), new_len) }
    }
}
