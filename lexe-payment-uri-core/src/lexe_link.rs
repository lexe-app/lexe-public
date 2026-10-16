use std::{borrow::Cow, fmt};

use lexe_uri::Uri;
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{Error, PaymentUri, PermittedMethods};

/// A Lexe pay app link, which opens the payment code it wraps directly in the
/// Lexe app, or on a lexe.app fallback page without it. For apps or sites that
/// want to send the user to the Lexe app specifically.
///
/// ex: `https://lexe.app/pay?to=lightning%3Alnbc1pvjlue...`
#[derive(Debug)]
#[cfg_attr(test, derive(Arbitrary, Eq, PartialEq))]
pub struct LexePayUrl {
    /// Percent-encoded in the `to` param. Never another Lexe link.
    #[cfg_attr(
        test,
        proptest(strategy = "PaymentUri::any_unwrapped().prop_map(Box::new)")
    )]
    pub payment_uri: Box<PaymentUri>,
}

/// A `lexe:` URI deep link, which opens the payment code it wraps directly in
/// the Lexe app on the same device, rather than in any wallet that handles e.g.
/// "bitcoin:". Not as secure as a [`LexePayUrl`], since a malicious app can
/// also register as a `lexe:` URI handler.
///
/// ex: "lexe:lightning:lnbc1pvjlue..." or
///     "lexe://lightning:lnbc1pvjlue..."
#[derive(Debug)]
#[cfg_attr(test, derive(Arbitrary, Eq, PartialEq))]
pub struct LexeUri {
    /// Never another Lexe link.
    #[cfg_attr(
        test,
        proptest(strategy = "PaymentUri::any_unwrapped().prop_map(Box::new)")
    )]
    pub payment_uri: Box<PaymentUri>,
}

// --- impl LexePayUrl --- //

impl LexePayUrl {
    /// Only allow payments in `https://lexe.app/pay` links.
    pub const PERMITTED_METHODS: PermittedMethods = PermittedMethods::PAYMENT;

    const BASE_URL: &'static str = "https://lexe.app/pay";

    pub fn parse(s: &str) -> Result<Self, Error> {
        let s = s.trim();
        if !Self::matches(s) {
            return Err(Error::InvalidPaymentUri(Cow::from(
                "Not a 'https://lexe.app/pay' link",
            )));
        }

        let uri = Uri::parse(s).map_err(Error::InvalidUri)?;
        let payment_code = uri
            .params
            .into_iter()
            .find(|param| param.key == "to")
            .map_or(Cow::Borrowed(""), |param| param.value);
        helpers::parse_lexe_wrapped(&payment_code)
            .map(|payment_uri| Self { payment_uri })
    }

    /// Whether `s` is a `https://lexe.app/pay` link.
    pub(crate) fn matches(s: &str) -> bool {
        s.strip_prefix(Self::BASE_URL)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('?'))
    }

    fn to_url(&self) -> Uri<'static> {
        let mut url =
            Uri::parse(Self::BASE_URL).expect("BASE_URL is a valid uri");
        url.push_param("to", self.payment_uri.to_string());
        url
    }
}

impl fmt::Display for LexePayUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_url(), f)
    }
}

// --- impl LexeUri --- //

impl LexeUri {
    /// Any app can register a `lexe:` scheme and hijack our links, so don't
    /// allow LexeConnect here. See "Request hijacking" in
    /// lexe-connect/README.md.
    pub const PERMITTED_METHODS: PermittedMethods =
        PermittedMethods::PAYMENT_AND_CLAIM;

    const URI_SCHEME: &'static str = "lexe";

    pub fn parse(s: &str) -> Result<Self, Error> {
        let payment_code =
            Self::strip_uri_scheme(s.trim()).ok_or_else(|| {
                Error::InvalidPaymentUri(Cow::from("Not a 'lexe:' URI"))
            })?;
        helpers::parse_lexe_wrapped(payment_code)
            .map(|payment_uri| Self { payment_uri })
    }

    /// Whether `s` is a `lexe:` or `lexe://` URI.
    pub(crate) fn matches(s: &str) -> bool {
        Self::strip_uri_scheme(s).is_some()
    }

    /// Strips the `lexe:` or `lexe://` scheme, returning the payment code.
    /// Unlike [`Uri::body`], this keeps the payment code's own query string.
    fn strip_uri_scheme(s: &str) -> Option<&str> {
        let (scheme, payment_code) = s.split_once(':')?;
        if !scheme.eq_ignore_ascii_case(Self::URI_SCHEME) {
            return None;
        }
        Some(payment_code.strip_prefix("//").unwrap_or(payment_code))
    }
}

impl fmt::Display for LexeUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = Self::URI_SCHEME;
        let payment_uri = &self.payment_uri;
        write!(f, "{scheme}:{payment_uri}")
    }
}

mod helpers {
    use super::*;

    /// Parse the payment code inside a [`LexePayUrl`] or [`LexeUri`].
    pub(super) fn parse_lexe_wrapped(
        s: &str,
    ) -> Result<Box<PaymentUri>, Error> {
        if s.is_empty() {
            return Err(Error::InvalidPaymentUri(Cow::from(
                "Lexe link is missing a payment code",
            )));
        }
        if LexePayUrl::matches(s) || LexeUri::matches(s) {
            return Err(Error::InvalidPaymentUri(Cow::from(
                "Lexe links can't be nested",
            )));
        }
        PaymentUri::parse_unwrapped(s).map(Box::new)
    }
}

#[cfg(test)]
mod test {
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    /// A BIP321 URI with its own query params.
    const BIP321: &str = "bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU\
        ?amount=50&label=Luke-Jr";
    /// [`BIP321`] in a [`LexePayUrl`], percent-encoded.
    const BIP321_PAY_URL: &str = "https://lexe.app/pay\
        ?to=bitcoin%3A13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU\
        %3Famount%3D50%26label%3DLuke-Jr";

    #[test]
    fn test_lexe_pay_url_roundtrip() {
        proptest!(|(url1: LexePayUrl)| {
            let url2 = LexePayUrl::parse(&url1.to_string());
            prop_assert_eq!(Ok(&url1), url2.as_ref());
        });
    }

    #[test]
    fn test_lexe_uri_roundtrip() {
        proptest!(|(uri1: LexeUri)| {
            let uri2 = LexeUri::parse(&uri1.to_string());
            prop_assert_eq!(Ok(&uri1), uri2.as_ref());
        });
    }

    #[test]
    fn test_lexe_pay_url() {
        let expected = LexePayUrl {
            payment_uri: Box::new(PaymentUri::parse(BIP321).unwrap()),
        };

        // Other params are ignored, and the payment code renders encoded
        for s in [
            BIP321_PAY_URL.to_owned(),
            format!("{BIP321_PAY_URL}&utm_source=x"),
            BIP321_PAY_URL.replace("pay?", "pay?utm_source=x&"),
        ] {
            let url = LexePayUrl::parse(&s).unwrap();
            assert_eq!(url, expected);
            assert_eq!(url.to_string(), BIP321_PAY_URL);
        }

        for s in [
            // Not `/pay`
            "https://lexe.app/payments?to=13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU",
            // Missing a payment code
            "https://lexe.app/pay",
            "https://lexe.app/pay?to=",
            "https://lexe.app/pay?utm_source=x",
            // Nested
            "https://lexe.app/pay?to=lexe%3A13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU",
        ] {
            LexePayUrl::parse(s).unwrap_err();
        }
    }

    #[test]
    fn test_lexe_uri() {
        let bip321 = PaymentUri::parse(BIP321).unwrap();
        let expected = LexeUri {
            payment_uri: Box::new(PaymentUri::parse(BIP321).unwrap()),
        };

        // The payment code keeps its own query string, in any `lexe:` form
        for s in [
            format!("lexe:{BIP321}"),
            format!("lexe://{BIP321}"),
            format!("LEXE:{BIP321}"),
            format!("LEXE://{BIP321}"),
        ] {
            let uri = LexeUri::parse(&s).unwrap();
            assert_eq!(uri, expected);
            assert_eq!(uri.to_string(), format!("lexe:{bip321}"));
        }

        for s in [
            // Not `lexe:`
            BIP321.to_owned(),
            // Missing a payment code
            "lexe:".to_owned(),
            "lexe://".to_owned(),
            // Nested
            format!("lexe:lexe:{BIP321}"),
            format!("lexe:{BIP321_PAY_URL}"),
        ] {
            LexeUri::parse(&s).unwrap_err();
        }
    }
}
