//! A Bitcoin amount newtype which maintains some useful internal invariants and
//! provides utilities for conversions to and from common Bitcoin units.
//!
//! Note that we don't impl `From<u64>`, `TryFrom<Decimal>`, [`FromStr`], etc
//! because we want calling code to be explicit about what the input unit is.
//!
//! ### Parsing [`Amount`]s
//!
//! If an [`Amount`] needs to be parsed from a user-provided [`String`], use
//! `Decimal::from_str`, then call the appropriate [`Amount`] constructor.
//!
//! ```
//! # use common::ln::amount::Amount;
//! # use rust_decimal::Decimal;
//! # use std::str::FromStr;
//!
//! let sats_str = "42069";
//! let sats_dec = Decimal::from_str(sats_str).expect("Not a number");
//! let amount1 = Amount::try_from_sats(sats_dec).expect("Invalid amount");
//!
//! let btc_str = "42.069";
//! let btc_dec = Decimal::from_str(btc_str).expect("Not a number");
//! let amount2 = Amount::try_from_btc(btc_dec).expect("Invalid amount");
//! ```
//!
//! ### [`Display`]ing [`Amount`]s
//!
//! [`Amount`]'s [`Display`] impl displays the contained satoshi [`Decimal`]
//! value, respects [`std::fmt`] syntax, and does not include " sats" in the
//! output. If a different unit is desired, call the appropriate getter, then
//! use the outputted [`Decimal`]'s [`Display`] impl for equivalent behavior.
//!
//! ```
//! # use common::ln::amount::Amount;
//!
//! let amount = Amount::from_msat(69_420_420);
//! println!("{amount} msats");
//!
//! let sats = amount.sats();
//! println!("{sats:.3} satoshis");
//!
//! let btc = amount.btc();
//! println!("{btc:.8} BTC");
//! ```
//!
//! [`Display`]: std::fmt::Display
//! [`FromStr`]: std::str::FromStr
//! [`Amount`]: crate::ln::amount::Amount
//! [`Decimal`]: rust_decimal::Decimal

// When writing large satoshi-denominated values, it's easier to parse the
// fractional satoshi amounts when they're grouped differently from the whole
// bitcoin amounts.
//
// Ex: suppose we have "1,305.00250372 BTC". It's hard to parse the consistenly
// spaced 130_500_250_372 sats, vs 1_305_0025_0372, which groups the fractional
// sats portion differently.
#![allow(clippy::inconsistent_digit_grouping)]

use std::{
    fmt::{self, Display},
    iter::Sum,
    ops::{Add, AddAssign, Div, Mul, Sub},
    str::FromStr,
};

use anyhow::format_err;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde::{Deserialize, Deserializer, Serialize};

/// Errors that can occur when attempting to construct an [`Amount`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Amount is negative")]
    Negative,
    #[error("Amount is too large")]
    TooLarge,
}

/// A Bitcoin amount, internally represented as a satoshi [`Decimal`], which
/// provides the following properties:
///
/// - The contained value is non-negative.
/// - The contained value is no greater than [`Amount::MAX`].
/// - Converting to sats, bits, or BTC and back via divisions and
///   multiplications by 1000 doesn't lose any precision.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize)]
pub struct Amount(Decimal);

impl Amount {
    /// The maximum [`Amount`] that this type can represent. We set this exactly
    /// equal to [`u64::MAX`] millisatoshis because it makes conversions to and
    /// from [`u64`] infallible and hence ergonomic, desirable because [`u64`]
    /// is the most common representation for millisats in non-Lexe code.
    // Correctness of this Decimal::from_parts is checked in the tests
    pub const MAX: Self =
        Self(Decimal::from_parts(4294967295, 4294967295, 0, false, 3));

    /// An [`Amount`] of zero bitcoins.
    pub const ZERO: Self = Self(dec!(0));

    /// The maximum supply of Bitcoin that can ever exist. Analogous to
    /// [`bitcoin::Amount::MAX_MONEY`]; primarily useful as a sanity check.
    // 21 million BTC * 100 million sats per BTC.
    pub const MAX_BITCOIN_SUPPLY: Self = Self(dec!(21_000_000_0000_0000));
    pub const MAX_BITCOIN_SUPPLY_SATS_U64: u64 = 21_000_000_0000_0000;
    pub const MAX_BITCOIN_SUPPLY_MSATS_U64: u64 = 21_000_000_0000_0000_000;

    /// The maximum amount we can set in a BOLT11 invoice via the LDK
    /// [`lightning_invoice::InvoiceBuilder::amount_milli_satoshis`] API.
    /// Setting above this value will overflow!
    pub const INVOICE_MAX_AMOUNT_MSATS_U64: u64 = u64::MAX / 10;

    // --- Constructors --- //

    /// Construct an [`Amount`] from a millisatoshi [`u64`] value.
    #[inline]
    pub fn from_msat(msats: u64) -> Self {
        Self(Decimal::from(msats) / dec!(1000))
    }

    /// Construct an [`Amount`] from a satoshi [`u32`] value.
    #[inline]
    pub fn from_sats_u32(sats_u32: u32) -> Self {
        Self::from_msat(u64::from(sats_u32) * 1000)
    }

    /// Construct an [`Amount`] from a satoshi [`u64`] value.
    #[inline]
    pub fn try_from_sats_u64(sats_u64: u64) -> Result<Self, Error> {
        Self::try_from_sats(Decimal::from(sats_u64))
    }

    /// Construct an [`Amount`] from a satoshi [`Decimal`] value.
    #[inline]
    pub fn try_from_sats(sats: Decimal) -> Result<Self, Error> {
        Self::try_from_inner(sats)
    }

    /// Construct an [`Amount`] from a BTC [`Decimal`] value.
    #[inline]
    pub fn try_from_btc(btc: Decimal) -> Result<Self, Error> {
        Self::try_from_inner(btc * dec!(1_0000_0000))
    }

    // --- Getters --- //
    // We *could* add bits() and millibits() here, but do we really need to?

    /// Returns the [`Amount`] as a [`u64`] millisatoshi value.
    #[inline]
    pub fn msat(&self) -> u64 {
        (self.0 * dec!(1000))
            .to_u64()
            .expect("Amount::MAX == u64::MAX millisats")
    }

    /// Returns the [`Amount`] as a [`u64`] millisatoshi value, but safe to
    /// use when _building_ a BOLT11 lightning invoice.
    pub fn invoice_safe_msat(&self) -> Result<u64, Error> {
        let msat = self.msat();
        if msat <= Self::INVOICE_MAX_AMOUNT_MSATS_U64 {
            Ok(msat)
        } else {
            Err(Error::TooLarge)
        }
    }

    /// Returns the [`Amount`] as a [`u64`] satoshi value.
    #[inline]
    pub fn sats_u64(&self) -> u64 {
        self.sats().to_u64().expect("Msats fits => sats fits")
    }

    /// Returns the [`Amount`] as a [`Decimal`] satoshi value.
    #[inline]
    pub fn sats(&self) -> Decimal {
        self.0
    }

    /// Returns the [`Amount`] as a [`Decimal`] BTC value.
    #[inline]
    pub fn btc(&self) -> Decimal {
        self.0 / dec!(1_0000_0000)
    }

    /// Round the sub-satoshi-precision part of the decimal.
    pub fn round_sat(&self) -> Self {
        Self(self.0.round())
    }

    /// Returns the absolute difference |x-y| between two [`Amount`]s.
    #[inline]
    pub fn abs_diff(self, other: Self) -> Amount {
        if self >= other {
            self - other
        } else {
            other - self
        }
    }

    /// Returns true if two amounts are approximately equal, up to some
    /// `epsilon` max difference.
    #[inline]
    pub fn approx_eq(self, other: Self, epsilon: Self) -> bool {
        self.abs_diff(other) <= epsilon
    }

    // --- Checked arithmetic --- //

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        let inner = self.0.checked_add(rhs.0)?;
        Self::try_from_inner(inner).ok()
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        let inner = self.0.checked_sub(rhs.0)?;
        Self::try_from_inner(inner).ok()
    }

    // Amount * scalar => Amount
    pub fn checked_mul(self, rhs: Decimal) -> Option<Self> {
        let inner = self.0.checked_mul(rhs)?;
        Self::try_from_inner(inner).ok()
    }

    // Amount / scalar => Amount
    pub fn checked_div(self, rhs: Decimal) -> Option<Self> {
        let inner = self.0.checked_div(rhs)?;
        Self::try_from_inner(inner).ok()
    }

    /// Checks all internal invariants, returning [`Self`] if all were OK.
    #[inline]
    fn try_from_inner(inner: Decimal) -> Result<Self, Error> {
        if inner.is_sign_negative() {
            Err(Error::Negative)
        } else if inner > Self::MAX.0 {
            Err(Error::TooLarge)
        } else {
            Ok(Self(inner))
        }
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner: Decimal = Deserialize::deserialize(deserializer)?;

        Self::try_from_inner(inner).map_err(|e| match e {
            Error::Negative => serde::de::Error::custom("Amount was negative"),
            Error::TooLarge => serde::de::Error::custom("Amount was too large"),
        })
    }
}

impl Display for Amount {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate to Decimal's Display impl which respects `std::fmt` syntax.
        Decimal::fmt(&self.0, f)
    }
}

impl FromStr for Amount {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decimal =
            Decimal::from_str(s).map_err(|err| format_err!("{err}"))?;
        Ok(Amount::try_from_inner(decimal)?)
    }
}

// --- bitcoin::Amount conversions --- //
// `bitcoin::Amount` is represented as u64 *satoshis*, so a conversion *to*
// their type is infallible, while a conversion *from* their type is not.

impl From<Amount> for bitcoin::Amount {
    #[inline]
    fn from(amt: Amount) -> Self {
        Self::from_sat(amt.sats().to_u64().expect("safe by construction"))
    }
}

impl TryFrom<bitcoin::Amount> for Amount {
    type Error = Error;
    #[inline]
    fn try_from(amt: bitcoin::Amount) -> Result<Self, Self::Error> {
        Self::try_from_sats(Decimal::from(amt.to_sat()))
    }
}

// --- Basic std::ops impls --- //

impl Add for Amount {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::try_from_inner(self.0 + rhs.0).expect("Overflowed")
    }
}
impl AddAssign for Amount {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Amount {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::try_from_inner(self.0 - rhs.0).expect("Underflowed")
    }
}

// Amount * scalar => Amount
impl Mul<Decimal> for Amount {
    type Output = Self;
    fn mul(self, rhs: Decimal) -> Self::Output {
        Self::try_from_inner(self.0 * rhs).expect("Overflowed")
    }
}
// scalar * Amount => Amount
impl Mul<Amount> for Decimal {
    type Output = Amount;
    fn mul(self, rhs: Amount) -> Self::Output {
        Amount::try_from_inner(self * rhs.0).expect("Overflowed")
    }
}

// Amount / scalar => Amount
impl Div<Decimal> for Amount {
    type Output = Self;
    fn div(self, rhs: Decimal) -> Self::Output {
        Self::try_from_inner(self.0 / rhs).expect("Overflowed")
    }
}

impl Sum for Amount {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Amount::ZERO, Self::add)
    }
}

// --- Tests and test infra --- //

#[cfg(any(test, feature = "test-utils"))]
pub mod arb {
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    /// All possible millisat amounts (up to the BTC max supply).
    impl Arbitrary for Amount {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (0_u64..=Amount::MAX_BITCOIN_SUPPLY_MSATS_U64)
                .prop_map(Amount::from_msat)
                .boxed()
        }
    }

    /// Maximum satoshi-precision amounts for e.g. onchain payments.
    pub fn sats_amount() -> impl Strategy<Value = Amount> {
        (0_u64..=Amount::MAX_BITCOIN_SUPPLY_SATS_U64)
            .prop_map(|sats_u64| Amount::try_from_sats_u64(sats_u64).unwrap())
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use proptest::{arbitrary::any, prop_assert, prop_assert_eq, proptest};

    use super::*;
    use crate::{test_utils::arbitrary, Apply};

    /// Check the correctness of the associated constants.
    #[test]
    fn check_associated_constants() {
        // Check the usage of Decimal::from_parts to define Amount::MAX
        let max_u64_msat_in_sat = Decimal::from(u64::MAX) / dec!(1000);
        println!("{:?}", max_u64_msat_in_sat.unpack());
        assert_eq!(Amount::MAX, Amount(max_u64_msat_in_sat));

        assert_eq!(Amount::MAX.msat(), u64::MAX);
        assert_eq!(
            Amount::MAX_BITCOIN_SUPPLY.sats(),
            dec!(21_000_000) * dec!(100_000_000),
        );
        assert_eq!(
            Amount::MAX_BITCOIN_SUPPLY.msat(),
            21_000_000 * 100_000_000 * 1000,
        );
    }

    /// Tests that converting the [`u64`] msat provided by LDK into our
    /// [`Amount`] newtype and back does not lose any precision.
    #[test]
    fn no_msat_u64_precision_loss() {
        proptest!(|(msat1 in any::<u64>())| {
            let amount = Amount::from_msat(msat1);
            let msat2 = amount.msat();
            prop_assert_eq!(msat1, msat2);
        })
    }

    /// Tests that [`u32`] satoshis roundtrips to and from [`Amount`].
    #[test]
    fn sat_u32_roundtrips() {
        proptest!(|(sat1 in any::<u32>())| {
            let amount = Amount::from_sats_u32(sat1);
            let sat2a = amount.sats_u64().apply(u32::try_from).unwrap();
            let sat2b = amount.sats().to_u32().unwrap();
            prop_assert_eq!(sat1, sat2a);
            prop_assert_eq!(sat1, sat2b);
        })
    }

    /// Tests that converting to fractional units like satoshis or BTC and back
    /// (using base 10 multiplications and divisions) does not lose precision,
    /// regardless of if it was done 'inside' or 'outside' the [`Amount`] impl.
    // 'Inside' refers to arithmetic done inside the getters and constructors;
    // 'Outside' refers to arithmetic done on the returned `Decimal` struct,
    // i.e. 'outside' of the Amount impls.
    #[test]
    fn no_roundtrip_inside_outside_precision_loss() {
        proptest!(|(amount in any::<Amount>())| {
            {
                // Roundtrip 'inside': Amount -> sat dec -> Amount
                let roundtrip_inside =
                    Amount::try_from_sats(amount.sats()).unwrap();
                prop_assert_eq!(amount, roundtrip_inside);

                // Roundtrip 'outside':
                // Amount -> msat u64 -> msat dec -> sat dec -> Amount
                let msat_u64 = amount.msat();
                let msat_dec = Decimal::from(msat_u64);
                let sat_dec = msat_dec / dec!(1000);
                let roundtrip_outside = Amount::try_from_sats(sat_dec).unwrap();
                prop_assert_eq!(roundtrip_inside, roundtrip_outside);
            }

            // Now do the same thing, but with the conversion to BTC.
            {
                // 'inside': Amount -> btc dec -> Amount
                let roundtrip_inside = Amount::try_from_btc(amount.btc()).unwrap();
                prop_assert_eq!(amount, roundtrip_inside);

                // 'outside': Amount -> msat u64 -> msat dec -> btc dec -> Amount
                let msat_u64 = amount.msat();
                let msat_dec = Decimal::from(msat_u64);
                let btc_dec = msat_dec / dec!(100_000_000_000);
                let roundtrip_outside = Amount::try_from_btc(btc_dec).unwrap();
                prop_assert_eq!(roundtrip_inside, roundtrip_outside);
            }
        })
    }

    /// Test the `Add` and `Sub` impls a bit.
    #[test]
    fn amount_add_sub() {
        proptest!(|(
            amount1 in any::<Amount>(),
            amount2 in any::<Amount>(),
        )| {
            let (greater, lesser) = if amount1 >= amount2 {
                (amount1, amount2)
            } else {
                (amount2, amount1)
            };

            let diff = greater - lesser;
            prop_assert_eq!(greater, lesser + diff);
            prop_assert_eq!(lesser, greater - diff);

            let checked_diff = greater.checked_sub(lesser).unwrap();
            prop_assert_eq!(greater, lesser.checked_add(checked_diff).unwrap());
            prop_assert_eq!(lesser, greater.checked_sub(checked_diff).unwrap());

            if greater > lesser {
                prop_assert!(lesser.checked_sub(greater).is_none());
                prop_assert!(Amount::MAX.checked_add(greater).is_none());
            }

            // Should never underflow
            prop_assert!(amount1.abs_diff(amount2) >= Amount::ZERO);
        })
    }

    /// Test the `Mul` and `Div` impls a bit.
    #[test]
    fn amount_mul_div() {
        proptest!(|(amount1 in any::<Amount>())| {
            let intermediate = amount1 / dec!(10);

            let amount2_scalar_first = dec!(10) * intermediate;
            prop_assert_eq!(amount1, amount2_scalar_first);
            let amount2_amount_first = intermediate * dec!(10);
            prop_assert_eq!(amount1, amount2_amount_first);

            let checked_int = amount1.checked_div(dec!(10)).unwrap();
            let checked_amount2 = checked_int.checked_mul(dec!(10)).unwrap();
            prop_assert_eq!(amount1, checked_amount2);
        })
    }

    /// Test rounding to the nearest satoshi.
    #[test]
    fn amount_round_sat_btc() {
        //
        // All whole sats values are unaffected by sats-rounding.
        //

        fn expect_no_precision_loss(amount: Amount) {
            assert_eq!(amount.btc(), amount.round_sat().btc());
        }

        expect_no_precision_loss(Amount::from_sats_u32(0));
        expect_no_precision_loss(Amount::from_sats_u32(10_0000));
        expect_no_precision_loss(Amount::from_sats_u32(10_0010_0005));
        expect_no_precision_loss(
            Amount::try_from_sats_u64(20_999_999_9999_9999).unwrap(),
        );

        proptest!(|(amount_u64: u64)| {
            // make all generated values representable
            let amount_u64 = amount_u64 % 2_100_000_000_000_000;
            let amount = Amount::try_from_sats_u64(amount_u64).unwrap();
            expect_no_precision_loss(amount);
        });

        //
        // sub-satoshi decimal part gets rounded
        //

        assert_eq!(Amount::from_msat(1).round_sat().btc(), Amount::ZERO.btc());
        assert_eq!(
            Amount::from_msat(1_001).round_sat().btc(),
            Amount::from_sats_u32(1).btc(),
        );
        assert_eq!(
            Amount::from_msat(1_501).round_sat().btc(),
            Amount::from_sats_u32(2).btc(),
        );
    }

    /// Test parsing BTC-denominated decimal values.
    #[test]
    fn amount_btc_str() {
        fn parse_btc_str(input: &str) -> Option<Amount> {
            Decimal::from_str(input)
                .ok()
                .and_then(|btc_decimal| Amount::try_from_btc(btc_decimal).ok())
        }
        fn parse_eq(input: &str, expected: Amount) {
            assert_eq!(parse_btc_str(input).unwrap(), expected);
        }
        fn parse_fail(input: &str) {
            if let Some(amount) = parse_btc_str(input) {
                panic!(
                    "Should fail to parse BTC str: '{input}', got: {amount:?}"
                );
            }
        }

        // These should parse correctly.

        parse_eq("0", Amount::ZERO);
        parse_eq("0.", Amount::ZERO);
        parse_eq(".0", Amount::ZERO);
        parse_eq("0.001", Amount::from_sats_u32(10_0000));
        parse_eq("10.00", Amount::from_sats_u32(10_0000_0000));
        parse_eq("10.", Amount::from_sats_u32(10_0000_0000));
        parse_eq("10", Amount::from_sats_u32(10_0000_0000));
        parse_eq("10.00000000", Amount::from_sats_u32(10_0000_0000));
        parse_eq("10.00001230", Amount::from_sats_u32(10_0000_1230));
        parse_eq("10.69696969", Amount::from_sats_u32(10_6969_6969));
        parse_eq("0.00001230", Amount::from_sats_u32(1230));
        parse_eq("0.69696969", Amount::from_sats_u32(6969_6969));
        parse_eq(".00001230", Amount::from_sats_u32(1230));
        parse_eq(".69696969", Amount::from_sats_u32(6969_6969));
        parse_eq(
            "20000000",
            Amount::try_from_sats_u64(20_000_000_0000_0000).unwrap(),
        );
        parse_eq(
            "20999999.99999999",
            Amount::try_from_sats_u64(20_999_999_9999_9999).unwrap(),
        );

        // These should not parse.

        parse_fail(".");
        parse_fail("asdif.");
        parse_fail("156.(6kfjaosid");
        parse_fail("-156");
        parse_fail("-15.4984");
        parse_fail("-.4");
        parse_fail(" 0.4");
        parse_fail("0.4 ");

        // Amounts should roundtrip: Amount -> BTC decimal string -> Amount.

        proptest!(|(amount: Amount)| {
            let amount_btc_str = amount.btc().to_string();
            let amount_round_sat_btc_str = amount.round_sat().btc().to_string();
            let amount_btc_str_btc = parse_btc_str(&amount_btc_str).unwrap();
            let amount_round_sat_btc_str_btc = parse_btc_str(&amount_round_sat_btc_str).unwrap();
            prop_assert_eq!(amount, amount_btc_str_btc);
            prop_assert_eq!(amount.btc(), amount_btc_str_btc.btc());
            prop_assert_eq!(amount.round_sat(), amount_round_sat_btc_str_btc);
            prop_assert_eq!(amount.round_sat().btc(), amount_round_sat_btc_str_btc.btc());
        });

        // Should never panic parsing any strings.

        proptest!(|(s in arbitrary::any_string())| {
            let _ = parse_btc_str(&s);
        });
    }
}
