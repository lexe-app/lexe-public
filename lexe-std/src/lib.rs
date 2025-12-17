//! # `lexe-std`
//!
//! This crate contains "std extensions" which other Lexe crates can use without
//! having to pull in any dependencies.
//!
//! Traits, macros, copies of unstable `std` APIs, a small number of types, are
//! all fair game so long as they do NOT depend on anything outside of [`std`].

// Re-export `std` for use by macros.
#[doc(hidden)]
pub use std;

/// `[u8; N]` array functions.
pub mod array;
/// Exponential backoff.
pub mod backoff;
/// Utilities for use in `const` fns and expressions.
pub mod const_utils;
/// `fmt` extensions.
pub mod fmt;
/// Iterator extensions.
pub mod iter;
/// Path extensions.
pub mod path;

/// A trait which allows us to apply functions (including tuple enum variants)
/// to non-[`Iterator`]/[`Result`]/[`Option`] values for cleaner iterator-like
/// chains. It exposes an [`apply`] method and is implemented for all `T`.
///
/// For example, instead of this:
///
/// ```ignore
/// # use common::ln::amount::Amount;
/// let value_sat_u64 = 100_000u64; // Pretend this is from LDK
/// let value_sat_u32 = u32::try_from(value_sat_u64)
///     .expect("Amount shouldn't have overflowed");
/// let maybe_value = Some(Amount::from_sats_u32(value_sat_u32));
/// ```
///
/// We can remove the useless `value_sat_u32` intermediate variable:
///
/// ```ignore
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
/// ```ignore
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
