//! A "parts per million" (ppm) newtype for proportional fee rates.
//!
//! PPM values represent a proportion where 1_000_000 ppm = 100%.
//! Valid range: 0 to 1_000_000 inclusive.
//!
//! ### Calculating fees
//!
//! Multiply an [`Amount`](crate::ln::amount::Amount) by a
//! [`Ppm`](crate::ppm::Ppm) to get the fee:
//!
//! ```
//! # use lexe_common::ppm::Ppm;
//! # use lexe_common::ln::amount::Amount;
//! let amount = Amount::from_sats_u32(100_000);
//! let fee_rate = Ppm::new(3000); // 0.3%
//! let fee = amount * fee_rate;
//! assert_eq!(fee, Amount::from_sats_u32(300));
//! ```
//!
//! ### Defining constants
//!
//! Use [`Ppm::new`](crate::ppm::Ppm::new) for compile-time validated constants:
//!
//! ```
//! # use lexe_common::ppm::Ppm;
//! const MY_FEE_RATE: Ppm = Ppm::new(3000); // 0.3%
//! ```
//!
//! ### Converting to a decimal rate
//!
//! [`Ppm::to_decimal`](crate::ppm::Ppm::to_decimal) returns a
//! [`Decimal`](rust_decimal::Decimal) rate:
//!
//! ```
//! # use lexe_common::ppm::Ppm;
//! # use lexe_common::dec;
//! let rate = Ppm::new(5000).to_decimal(); // 0.5%
//! assert_eq!(rate, dec!(0.005));
//! ```

use std::{fmt, ops::Mul, str::FromStr};

use anyhow::format_err;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{dec, ln::amount::Amount};

/// Errors that can occur when constructing a [`Ppm`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Ppm value is negative")]
    Negative,
    #[error("Ppm value exceeds 1_000_000")]
    TooLarge,
}

/// A "parts per million" value for proportional fee rates.
///
/// Internally stores an `i32` in the range `[0, 1_000_000]`.
/// 1_000_000 ppm = 100%, so 5000 ppm = 0.5%.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[serde(try_from = "i32", into = "i32")]
pub struct Ppm(i32);

impl Ppm {
    /// The maximum [`Ppm`] value (1_000_000 = 100%).
    pub const MAX: Self = Self(1_000_000);

    /// A [`Ppm`] of zero.
    pub const ZERO: Self = Self(0);

    /// Construct a [`Ppm`] from an `i32` value.
    ///
    /// # Panics
    ///
    /// Panics at compile time (in const context) or runtime if `value` is
    /// outside the valid range `[0, 1_000_000]`.
    #[inline]
    pub const fn new(value: i32) -> Self {
        assert!(value >= 0, "Ppm value must be non-negative");
        assert!(value <= Self::MAX.0, "Ppm value must be <= 1_000_000");
        Self(value)
    }

    /// Returns the ppm value as an `i32`.
    #[inline]
    pub const fn to_i32(self) -> i32 {
        self.0
    }

    /// Returns the ppm value as a `u32`.
    #[inline]
    pub const fn to_u32(self) -> u32 {
        self.0 as u32
    }

    /// Returns the ppm value as a [`Decimal`] rate (ppm / 1_000_000).
    ///
    /// For example, 5000 ppm becomes `0.005` (0.5%).
    #[inline]
    pub fn to_decimal(self) -> Decimal {
        Decimal::from(self.0) / dec!(1_000_000)
    }

    /// Checks bounds, returning [`Self`] if the value is valid.
    #[inline]
    fn try_from_inner(value: i32) -> Result<Self, Error> {
        if value < 0 {
            Err(Error::Negative)
        } else if value > Self::MAX.0 {
            Err(Error::TooLarge)
        } else {
            Ok(Self(value))
        }
    }
}

impl fmt::Display for Ppm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Ppm {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.parse::<i32>().map_err(|err| format_err!("{err}"))?;
        Ok(Self::try_from_inner(value)?)
    }
}

// --- Infallible From impls --- //

impl From<u16> for Ppm {
    /// Infallible conversion from `u16` (max 65535 < 1_000_000).
    #[inline]
    fn from(value: u16) -> Self {
        Self(i32::from(value))
    }
}

impl From<Ppm> for i32 {
    #[inline]
    fn from(ppm: Ppm) -> Self {
        ppm.0
    }
}

impl From<Ppm> for u32 {
    #[inline]
    fn from(ppm: Ppm) -> Self {
        ppm.0 as u32
    }
}

impl From<Ppm> for i64 {
    #[inline]
    fn from(ppm: Ppm) -> Self {
        i64::from(ppm.0)
    }
}

impl From<Ppm> for u64 {
    #[inline]
    fn from(ppm: Ppm) -> Self {
        ppm.0 as u64
    }
}

// --- Fallible TryFrom impls --- //

impl TryFrom<i32> for Ppm {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::try_from_inner(value)
    }
}

impl TryFrom<u32> for Ppm {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let value_i32 = i32::try_from(value).map_err(|_| Error::TooLarge)?;
        Self::try_from_inner(value_i32)
    }
}

impl TryFrom<Decimal> for Ppm {
    type Error = Error;

    /// Construct a [`Ppm`] from a [`Decimal`] rate.
    ///
    /// The decimal is multiplied by 1_000_000 and rounded to the nearest
    /// integer. For example, `0.005` (0.5%) becomes 5000 ppm.
    ///
    /// Returns an error if the result is negative or exceeds 1_000_000.
    fn try_from(rate: Decimal) -> Result<Self, Self::Error> {
        use rust_decimal::prelude::ToPrimitive;

        let ppm_dec = (rate * dec!(1_000_000)).round();
        let ppm_i32 = ppm_dec.to_i32().ok_or(Error::TooLarge)?;
        Self::try_from_inner(ppm_i32)
    }
}

// --- Mul impls for fee calculation --- //
//
// These impls can never panic: Ppm is bounded to [0, 1_000_000] representing
// [0%, 100%], so multiplying a valid Amount by a Ppm always produces a result
// ≤ the original Amount.

/// Amount * Ppm => Amount (fee calculation)
impl Mul<Ppm> for Amount {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Ppm) -> Self::Output {
        self * rhs.to_decimal()
    }
}

/// Ppm * Amount => Amount (fee calculation, commutative)
impl Mul<Amount> for Ppm {
    type Output = Amount;

    #[inline]
    fn mul(self, rhs: Amount) -> Self::Output {
        rhs * self.to_decimal()
    }
}

// --- Arbitrary impl --- //

#[cfg(any(test, feature = "test-utils"))]
impl proptest::arbitrary::Arbitrary for Ppm {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;
        (0i32..=Self::MAX.0).prop_map(Self).boxed()
    }
}

// --- Tests --- //

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assert, prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn const_construction() {
        /// Test const construction.
        const TEST_PPM: Ppm = Ppm::new(3000);

        assert_eq!(TEST_PPM.to_i32(), 3000);
        assert_eq!(Ppm::ZERO.to_i32(), 0);
        assert_eq!(Ppm::MAX.to_i32(), 1_000_000);
    }

    #[test]
    fn to_decimal() {
        assert_eq!(Ppm::ZERO.to_decimal(), dec!(0));
        assert_eq!(Ppm::new(1).to_decimal(), dec!(0.000001));
        assert_eq!(Ppm::new(1000).to_decimal(), dec!(0.001));
        assert_eq!(Ppm::new(10_000).to_decimal(), dec!(0.01));
        assert_eq!(Ppm::new(100_000).to_decimal(), dec!(0.1));
        assert_eq!(Ppm::MAX.to_decimal(), dec!(1));
    }

    #[test]
    fn try_from_decimal() {
        // Basic conversions
        assert_eq!(Ppm::try_from(dec!(0)).unwrap(), Ppm::ZERO);
        assert_eq!(Ppm::try_from(dec!(0.005)).unwrap(), Ppm::new(5000));
        assert_eq!(Ppm::try_from(dec!(0.1)).unwrap(), Ppm::new(100_000));
        assert_eq!(Ppm::try_from(dec!(1)).unwrap(), Ppm::MAX);

        // Rounding
        assert_eq!(Ppm::try_from(dec!(0.0000014)).unwrap(), Ppm::new(1));
        assert_eq!(Ppm::try_from(dec!(0.0000016)).unwrap(), Ppm::new(2));

        // Errors
        assert!(matches!(Ppm::try_from(dec!(-0.001)), Err(Error::Negative)));
        assert!(matches!(
            Ppm::try_from(dec!(1.000001)),
            Err(Error::TooLarge)
        ));
    }

    #[test]
    fn try_from_rejects_invalid() {
        assert!(matches!(Ppm::try_from(-1i32), Err(Error::Negative)));
        assert!(matches!(Ppm::try_from(1_000_001i32), Err(Error::TooLarge)));
        assert!(matches!(Ppm::try_from(1_000_001u32), Err(Error::TooLarge)));
    }

    #[test]
    fn from_str() {
        assert_eq!("0".parse::<Ppm>().unwrap(), Ppm::ZERO);
        assert_eq!("3000".parse::<Ppm>().unwrap(), Ppm::new(3000));
        assert_eq!("1000000".parse::<Ppm>().unwrap(), Ppm::MAX);

        assert!("-1".parse::<Ppm>().is_err());
        assert!("1000001".parse::<Ppm>().is_err());
        assert!("abc".parse::<Ppm>().is_err());
    }

    /// Verifies JSON format is a bare integer, not an object.
    #[test]
    fn serde_json_format() {
        #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
        struct Foo {
            ppm: Ppm,
        }

        let foo = Foo {
            ppm: Ppm::new(3000),
        };
        let json = serde_json::to_string(&foo).unwrap();
        assert_eq!(json, r#"{"ppm":3000}"#);
        let roundtrip: Foo = serde_json::from_str(&json).unwrap();
        assert_eq!(foo, roundtrip);

        // Rejects invalid values
        assert!(serde_json::from_str::<Ppm>("-1").is_err());
        assert!(serde_json::from_str::<Ppm>("1000001").is_err());
    }

    #[test]
    fn proptest_integer_conversions() {
        proptest!(|(ppm in any::<Ppm>(), val in any::<u16>())| {
            let i = ppm.to_i32();

            // All integer conversions agree
            prop_assert_eq!(i32::from(ppm), i);
            prop_assert_eq!(u32::from(ppm), i as u32);
            prop_assert_eq!(i64::from(ppm), i64::from(i));
            prop_assert_eq!(u64::from(ppm), i as u64);

            // TryFrom roundtrips
            prop_assert_eq!(Ppm::try_from(i).unwrap(), ppm);
            prop_assert_eq!(Ppm::try_from(i as u32).unwrap(), ppm);

            // From<u16> always succeeds (max 65535 < 1_000_000)
            let from_u16 = Ppm::from(val);
            prop_assert_eq!(from_u16.to_i32(), i32::from(val));
        });
    }

    #[test]
    fn proptest_mul_amount() {
        proptest!(|(amount in any::<Amount>(), ppm in any::<Ppm>())| {
            // Commutative: amount * ppm == ppm * amount
            prop_assert_eq!(amount * ppm, ppm * amount);

            // Equivalent to multiplying by the decimal rate
            prop_assert_eq!(amount * ppm, amount * ppm.to_decimal());
        });
    }

    #[test]
    fn proptest_serde_roundtrip() {
        proptest!(|(ppm in any::<Ppm>())| {
            let json = serde_json::to_string(&ppm).unwrap();
            let roundtrip: Ppm = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(ppm, roundtrip);
        });
    }

    #[test]
    fn proptest_decimal_roundtrip() {
        proptest!(|(ppm in any::<Ppm>())| {
            let dec = ppm.to_decimal();

            // Decimal is in [0, 1]
            prop_assert!(dec >= Decimal::ZERO);
            prop_assert!(dec <= Decimal::ONE);

            // Roundtrip: Ppm -> Decimal -> Ppm
            let roundtrip = Ppm::try_from(dec).unwrap();
            prop_assert_eq!(ppm, roundtrip);
        });
    }
}
