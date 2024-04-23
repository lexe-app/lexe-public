//! Data types returned from the fiat exchange rate API.

use std::{borrow::Borrow, collections::BTreeMap, fmt};

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::time::TimestampMs;

/// Fiat currency ISO 4217 code.
///
/// ### Examples
///
/// `"USD", "EUR", "DKK", "CNY", ...`
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(transparent)]
pub struct FiatCode(pub String);

/// The BTC price in a given fiat currency.
///
/// We just return this as an `f64`, which is kind of haram but also super
/// convenient. Fortunately, all our accounting is done using BTC and we only
/// use these exchange rates for display purposes, so it's probably OK?
#[derive(Clone, Copy, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct FiatRates {
    /// The unix timestamp of the fiat<->BTC exchange rate quotes from the
    /// upstream data source.
    pub timestamp_ms: TimestampMs,
    /// A mapping from fiat symbol (e.g., "USD", "EUR") to the current BTC
    /// price in that fiat currency (e.g., "USD" => $28,401.98 per BTC).
    ///
    /// We store and serialize this map in sorted order so it's easier to scan.
    pub rates: BTreeMap<FiatCode, FiatBtcPrice>,
}

impl FiatRates {
    pub fn dummy() -> Self {
        Self {
            timestamp_ms: TimestampMs::now(),
            rates: BTreeMap::from_iter([
                (FiatCode("USD".to_owned()), FiatBtcPrice(67086.56654977065)),
                (FiatCode("EUR".to_owned()), FiatBtcPrice(62965.97545915064)),
            ]),
        }
    }
}

// --- impl FiatCode --- //

impl Borrow<str> for FiatCode {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for FiatCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// --- impl FiatBtcPrice --- //

impl fmt::Debug for FiatBtcPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use std::str;

    use proptest::{
        array::uniform3,
        prelude::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::FiatCode;

    impl Arbitrary for FiatCode {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            uniform3(b'A'..=b'Z')
                .prop_map(|code| {
                    FiatCode(str::from_utf8(&code).unwrap().to_owned())
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::FiatRates;
    use crate::test_utils::roundtrip::json_value_roundtrip_proptest;

    #[test]
    fn fiat_rates_roundtrip() {
        json_value_roundtrip_proptest::<FiatRates>();
    }
}
