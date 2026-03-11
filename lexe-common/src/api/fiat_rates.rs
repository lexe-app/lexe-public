//! Data types returned from the fiat exchange rate API.

use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use lexe_std::const_utils::const_result_unwrap;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serde_with::DeserializeFromStr;

use crate::time::TimestampMs;

/// Currency ISO 4217 code. Intended to _only_ cover fiat currencies. For our
/// purposes, a fiat currency code is _always_ three uppercase ASCII characters
/// (i.e., `[A-Z]{3}`).
///
/// ### Examples
///
/// `"USD", "EUR", "DKK", "CNY", ...`
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(DeserializeFromStr)]
pub struct IsoCurrencyCode([u8; 3]);

/// The BTC price in a given fiat currency.
///
/// We just return this as an `f64`, which is kind of haram but also super
/// convenient. Fortunately, all our accounting is done using BTC and we only
/// use these exchange rates for display purposes, so it's probably OK?
#[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct FiatBtcPrice(pub f64);

/// A quote for various fiat<->BTC exchange rates.
///
/// The mobile app client always requests the full set of exchange rates, since
/// the serialized, uncompressed size is not too big (~2.5 KiB). Using the full
/// set reduces some client complexity and is easier to cache.
///
/// ### Example
///
/// ```json
/// {
///     "timestamp_ms": 1680228982999,
///     "rates": {
///         "EUR": 26168.988183514073,
///         "USD": 28401.980274690515,
///         // ..
///     }
/// }
/// ```
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct FiatRates {
    /// The unix timestamp of the fiat<->BTC exchange rate quotes from the
    /// upstream data source.
    pub timestamp_ms: TimestampMs,
    /// A mapping from fiat symbol (e.g., "USD", "EUR") to the current BTC
    /// price in that fiat currency (e.g., "USD" => $28,401.98 per BTC).
    ///
    /// We store and serialize this map in sorted order so it's easier to scan.
    pub rates: BTreeMap<IsoCurrencyCode, FiatBtcPrice>,
}

/// An error from parsing an [`IsoCurrencyCode`].
#[derive(Copy, Clone, Debug)]
pub enum ParseError {
    BadLength,
    BadCharacter,
}

// --- impl FiatRates --- //

impl FiatRates {
    pub fn dummy() -> Self {
        Self {
            timestamp_ms: TimestampMs::now(),
            rates: BTreeMap::from_iter([
                (IsoCurrencyCode::USD, FiatBtcPrice(67086.56654977065)),
                (IsoCurrencyCode::EUR, FiatBtcPrice(62965.97545915064)),
            ]),
        }
    }
}

// --- impl IsoCurrencyCode --- //

impl IsoCurrencyCode {
    pub const USD: Self = const_result_unwrap(Self::try_from_bytes(*b"USD"));
    pub const EUR: Self = const_result_unwrap(Self::try_from_bytes(*b"EUR"));
    // technically not a fiat, but useful
    pub const BTC: Self = const_result_unwrap(Self::try_from_bytes(*b"BTC"));

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: we guarantee that IsoCurrencyCode is always uppercase ASCII.
        unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
    }

    #[inline]
    const fn try_from_bytes(value: [u8; 3]) -> Result<Self, ParseError> {
        let [c0, c1, c2] = value;
        // Do it like this so we can use it in `const`
        if c0.is_ascii_uppercase()
            && c1.is_ascii_uppercase()
            && c2.is_ascii_uppercase()
        {
            Ok(Self(value))
        } else {
            Err(ParseError::BadCharacter)
        }
    }
}

impl FromStr for IsoCurrencyCode {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = <[u8; 3]>::try_from(s.as_bytes())
            .map_err(|_| ParseError::BadLength)?;
        Self::try_from_bytes(inner)
    }
}

// impl Borrow<str> for IsoCurrencyCode {
//     #[inline]
//     fn borrow(&self) -> &str {
//         self.as_str()
//     }
// }

impl fmt::Display for IsoCurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for IsoCurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl Serialize for IsoCurrencyCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

// --- impl ParseError --- //

impl ParseError {
    fn as_str(&self) -> &'static str {
        match *self {
            Self::BadLength =>
                "IsoCurrencyCode: must be exactly 3 characters long",
            Self::BadCharacter =>
                "IsoCurrencyCode: must be all uppercase ASCII",
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Error for ParseError {}

// --- impl FiatBtcPrice --- //

impl fmt::Debug for FiatBtcPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        array::uniform3,
        prelude::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::IsoCurrencyCode;

    impl Arbitrary for IsoCurrencyCode {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            uniform3(b'A'..=b'Z')
                .prop_map(|code| IsoCurrencyCode::try_from_bytes(code).unwrap())
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn json_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<IsoCurrencyCode>();
        roundtrip::json_value_roundtrip_proptest::<FiatRates>();
    }
}
