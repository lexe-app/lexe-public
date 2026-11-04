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

/// Defines supported fiat currencies.
///
/// ```ignore
/// supported_fiat_currencies! {
///     pub const SUPPORTED_FIAT_CURRENCIES: &[(IsoCurrencyCode, &str)] = &[
///         (IsoCurrencyCode::EUR, "Euro"),
///         (IsoCurrencyCode::USD, "US Dollar"),
///     ];
/// }
/// ```
///
/// emits the const as written, plus
///
/// ```ignore
/// impl IsoCurrencyCode {
///     pub const EUR: Self = /* "EUR" */;
///     pub const USD: Self = /* "USD" */;
/// }
/// ```
macro_rules! supported_fiat_currencies {
    (
        $(#[$meta:meta])*
        $vis:vis const $name:ident: &[(IsoCurrencyCode, &str)] = &[
            $((IsoCurrencyCode::$code:ident, $cname:literal)),* $(,)?
        ];
    ) => {
        impl IsoCurrencyCode {
            $(pub const $code: Self =
                const_result_unwrap(Self::try_from_str(stringify!($code)));)*
        }

        $(#[$meta])*
        $vis const $name: &[(IsoCurrencyCode, &str)] =
            &[$((IsoCurrencyCode::$code, $cname)),*];
    };
}

supported_fiat_currencies! {
    /// The fiat currencies Lexe supports, with their English names,
    /// sorted by code.
    pub const SUPPORTED_FIAT_CURRENCIES: &[(IsoCurrencyCode, &str)] = &[
        (IsoCurrencyCode::AED, "United Arab Emirates Dirham"),
        (IsoCurrencyCode::ARS, "Argentine Peso"),
        (IsoCurrencyCode::AUD, "Australian Dollar"),
        (IsoCurrencyCode::BRL, "Brazilian Real"),
        (IsoCurrencyCode::CAD, "Canadian Dollar"),
        (IsoCurrencyCode::CHF, "Swiss Franc"),
        (IsoCurrencyCode::CLP, "Chilean Peso"),
        (IsoCurrencyCode::CNY, "Chinese Yuan"),
        (IsoCurrencyCode::COP, "Colombian Peso"),
        (IsoCurrencyCode::CZK, "Czech Koruna"),
        (IsoCurrencyCode::DKK, "Danish Krone"),
        (IsoCurrencyCode::EGP, "Egyptian Pound"),
        (IsoCurrencyCode::EUR, "Euro"),
        (IsoCurrencyCode::GBP, "British Pound"),
        (IsoCurrencyCode::HKD, "Hong Kong Dollar"),
        (IsoCurrencyCode::HUF, "Hungarian Forint"),
        (IsoCurrencyCode::IDR, "Indonesian Rupiah"),
        (IsoCurrencyCode::ILS, "Israeli New Shekel"),
        (IsoCurrencyCode::INR, "Indian Rupee"),
        (IsoCurrencyCode::IQD, "Iraqi Dinar"),
        (IsoCurrencyCode::JPY, "Japanese Yen"),
        (IsoCurrencyCode::KRW, "South Korean Won"),
        (IsoCurrencyCode::KWD, "Kuwaiti Dinar"),
        (IsoCurrencyCode::KZT, "Kazakhstani Tenge"),
        (IsoCurrencyCode::LBP, "Lebanese Pound"),
        (IsoCurrencyCode::MAD, "Moroccan Dirham"),
        (IsoCurrencyCode::MOP, "Macanese Pataca"),
        (IsoCurrencyCode::MXN, "Mexican Peso"),
        (IsoCurrencyCode::MYR, "Malaysian Ringgit"),
        (IsoCurrencyCode::NGN, "Nigerian Naira"),
        (IsoCurrencyCode::NOK, "Norwegian Krone"),
        (IsoCurrencyCode::NZD, "New Zealand Dollar"),
        (IsoCurrencyCode::PEN, "Peruvian Sol"),
        (IsoCurrencyCode::PHP, "Philippine Peso"),
        (IsoCurrencyCode::PKR, "Pakistani Rupee"),
        (IsoCurrencyCode::PLN, "Polish Zloty"),
        (IsoCurrencyCode::QAR, "Qatari Riyal"),
        (IsoCurrencyCode::RON, "Romanian Leu"),
        (IsoCurrencyCode::RUB, "Russian Ruble"),
        (IsoCurrencyCode::SAR, "Saudi Riyal"),
        (IsoCurrencyCode::SEK, "Swedish Krona"),
        (IsoCurrencyCode::SGD, "Singapore Dollar"),
        (IsoCurrencyCode::THB, "Thai Baht"),
        (IsoCurrencyCode::TRY, "Turkish Lira"),
        (IsoCurrencyCode::TWD, "New Taiwan Dollar"),
        (IsoCurrencyCode::UAH, "Ukrainian Hryvnia"),
        (IsoCurrencyCode::USD, "US Dollar"),
        (IsoCurrencyCode::VND, "Vietnamese Dong"),
        (IsoCurrencyCode::ZAR, "South African Rand"),
    ];
}

// --- impl FiatRates --- //

impl FiatRates {
    pub fn dummy() -> Self {
        // One price per `SUPPORTED_FIAT_CURRENCIES` entry, in order.
        const PRICES: [f64; SUPPORTED_FIAT_CURRENCIES.len()] = [
            287962.69, 8.43e7, 131111.68, 463258.84, 111606.89, 67245.95,
            7.77e7, 573199.42, 3.36e8, 1.81e6, 535854.74, 4.03e6, 62965.98,
            61606.08, 609091.49, 2.92e7, 1.32e9, 296262.29, 6.74e6, 1.03e8,
            1.16e7, 1.15e8, 24141.14, 4.11e7, 7.03e9, 747744.64, 627681.03,
            1.62e6, 351232.82, 1.21e8, 860716.48, 141700.73, 288661.55, 4.49e6,
            2.20e7, 308179.20, 285908.85, 357167.71, 6.75e6, 294387.04,
            789538.13, 106025.19, 2.72e6, 2.98e6, 2.59e6, 3.22e6, 67086.57,
            2.02e9, 1.54e6,
        ];
        let rates = SUPPORTED_FIAT_CURRENCIES
            .iter()
            .zip(PRICES)
            .map(|((code, _name), price)| (*code, FiatBtcPrice(price)))
            .collect();
        Self {
            timestamp_ms: TimestampMs::now(),
            rates,
        }
    }
}

// --- impl IsoCurrencyCode --- //

impl IsoCurrencyCode {
    // technically not a fiat, but useful
    pub const BTC: Self = const_result_unwrap(Self::try_from_bytes(*b"BTC"));

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: we guarantee that IsoCurrencyCode is always uppercase ASCII.
        unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
    }

    /// [`FromStr::from_str`], but `const`.
    const fn try_from_str(s: &str) -> Result<Self, ParseError> {
        match s.as_bytes() {
            &[c0, c1, c2] => Self::try_from_bytes([c0, c1, c2]),
            _ => Err(ParseError::BadLength),
        }
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
        Self::try_from_str(s)
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
