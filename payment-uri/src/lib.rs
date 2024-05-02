//! Permissive decoding of bitcoin+lightning payment addresses+URIs.
//!
//! This module parses various BTC-related payment methods permissively. That
//! means we should parse inputs that are not strictly well-formed.

// TODO(phlip9): remove
#![allow(dead_code)]

use core::fmt;
use std::{borrow::Cow, str::FromStr};

use common::ln::{amount::Amount, invoice::LxInvoice};
#[cfg(test)]
use common::{ln::amount, test_utils::arbitrary};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;

// https://datatracker.ietf.org/doc/html/rfc3986#section-2.3
const BIP21_ASCII_SET: percent_encoding::AsciiSet =
    percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');

// TODO(phlip9): todo
//
// pub struct PaymentMethods(Vec<PaymentMethod>);
//
// impl PaymentMethods {
//     pub fn parse(s: &str) -> Vec<PaymentMethod> {
//         let s = s.trim();
//
//         if let Some(uri) = Uri::parse(s) {
//             return Self::parse_uri(uri);
//         }
//
//         Vec::new()
//     }
//
//     fn parse_uri(uri: Uri) -> Vec<PaymentMethod> {
//         if uri.scheme.eq_ignore_ascii_case("bitcoin") {
//             return Bip21Uri::parse_uri(uri)
//                 .map(|xs| xs.collect())
//                 .unwrap_or_default();
//         }
//
//         Vec::new()
//     }
// }

/// A single "payment method" -- each kind here should correspond with a single
/// linear payment flow for a user, where there are no other alternate methods.
///
/// For example, a Unified BTC QR code contains a single [`Bip21Uri`], which may
/// contain _multiple_ discrete payment methods (an onchain address, a BOLT11
/// invoice, a BOLT12 offer).
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(LxInvoice),
    // TODO(phlip9): BOLT12 offers
    // Offer()
}

/// An onchain payment method, usually parsed from a standalone BTC address or
/// BIP21 URI.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Onchain {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_mainnet_address()"))]
    address: bitcoin::Address,

    #[cfg_attr(
        test,
        proptest(strategy = "amount::arb::sats_amount().prop_map(Some)")
    )]
    amount: Option<Amount>,

    /// The recipient/payee name.
    label: Option<String>,

    /// The payment description.
    message: Option<String>,
}

/// Parse an onchain amount in BTC, e.g. "1.0024" => 1_0024_0000 sats. This
/// parser also rounds to the nearest satoshi amount, since on-chain payments
/// are limited to satoshi precision.
fn parse_onchain_btc_amount(s: &str) -> Option<Amount> {
    Decimal::from_str(s)
        .ok()
        .and_then(|btc_decimal| Amount::try_from_btc(btc_decimal).ok())
        // On-chain min. denomination
        .map(|amount| amount.round_sat())
}

/// A [BIP21 URI](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki).
/// Encodes an onchain address plus some extra metadata.
///
/// Wallets that use [Unified QRs](https://bitcoinqr.dev/) may also include a
/// BOLT11 invoice or BOLT12 offer as `lightning` or `b12` query params.
///
/// Examples:
///
/// ```not_rust
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?label=Luke-Jr
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=20.3&label=Luke-Jr
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?somethingyoudontunderstand=50&somethingelseyoudontget=999
/// ```
#[derive(Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Bip21Uri {
    onchain: Option<Onchain>,
    invoice: Option<LxInvoice>,
    // TODO(phlip9): BOLT12 offers
    // offer: Option<()>,
}

impl Bip21Uri {
    fn parse(s: &str) -> Option<Self> {
        let uri = Uri::parse(s)?;
        Self::parse_uri(uri)
    }

    fn parse_uri(uri: Uri) -> Option<Self> {
        if !uri.scheme.eq_ignore_ascii_case("bitcoin") {
            return None;
        }

        let mut out = Self {
            onchain: None,
            invoice: None,
        };

        // (Unified QR) Search for BOLT11 invoice and/or BOLT12 offer
        // <https://bitcoinqr.dev/>
        for param in &uri.params {
            match param.key.as_ref() {
                "lightning" if out.invoice.is_none() =>
                    out.invoice = LxInvoice::from_str(&param.value).ok(),

                // TODO(phlip9): BOLT12 offers
                // "b12" => if offer.is_none() => {}

                // ignore duplicates or other keys
                _ => {}
            }
        }

        // Can only parse the `Onchain` payment method if there's an address.
        if let Ok(address) = bitcoin::Address::from_str(&uri.body) {
            let mut amount = None;
            let mut label = None;
            let mut message = None;

            let mut skip = false;

            for param in uri.params {
                match param.key.as_ref() {
                    "amount" if amount.is_none() =>
                        amount = parse_onchain_btc_amount(&param.value),
                    "label" if label.is_none() =>
                        label = Some(param.value.into_owned()),
                    "message" if message.is_none() =>
                        message = Some(param.value.into_owned()),

                    // We'll respect required && unrecognized bip21 params by
                    // throwing out the whole onchain method.
                    _ if param.key.starts_with("req-") => {
                        skip = true;
                        break;
                    }
                    // ignore duplicates
                    _ => {}
                }
            }

            if !skip {
                out.onchain = Some(Onchain {
                    address,
                    amount,
                    label,
                    message,
                });
            }
        }

        Some(out)
    }

    fn to_uri(&self) -> Uri<'_> {
        let scheme = "bitcoin";
        let mut body = Cow::Borrowed("");
        let mut params = Vec::new();

        if let Some(onchain) = &self.onchain {
            body = Cow::Owned(onchain.address.to_string());

            if let Some(amount) = &onchain.amount {
                params.push(UriParam {
                    key: Cow::Borrowed("amount"),
                    // We need to round to satoshi-precision for this to be a
                    // valid on-chain amount.
                    value: Cow::Owned(amount.round_sat().btc().to_string()),
                });
            }

            if let Some(label) = &onchain.label {
                params.push(UriParam {
                    key: Cow::Borrowed("label"),
                    value: Cow::Borrowed(label),
                });
            }

            if let Some(message) = &onchain.message {
                params.push(UriParam {
                    key: Cow::Borrowed("message"),
                    value: Cow::Borrowed(message),
                });
            }
        }

        if let Some(invoice) = &self.invoice {
            params.push(UriParam {
                key: Cow::Borrowed("lightning"),
                value: Cow::Owned(invoice.to_string()),
            });
        }

        Uri {
            scheme,
            body,
            params,
        }
    }
}

impl Iterator for Bip21Uri {
    type Item = PaymentMethod;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(onchain) = self.onchain.take() {
            return Some(PaymentMethod::Onchain(onchain));
        }
        if let Some(invoice) = self.invoice.take() {
            return Some(PaymentMethod::Invoice(invoice));
        }
        // TODO(phlip9): BOLT12 offers
        None
    }
}

impl fmt::Display for Bip21Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_uri(), f)
    }
}

/// A raw, parsed URI. The params (both key and value) are percent-encoded. See
/// [URI syntax - RFC 3986](https://datatracker.ietf.org/doc/html/rfc3986).
///
/// ex: `http://example.com?foo=bar%20baz`
/// -> Uri {
///     scheme: "http",
///     body: "//example.com",
///     params: [("foo", "bar baz")],
/// }
#[derive(Debug)]
struct Uri<'a> {
    scheme: &'a str,
    body: Cow<'a, str>,
    params: Vec<UriParam<'a>>,
}

impl<'a> Uri<'a> {
    fn parse(s: &'a str) -> Option<Self> {
        // parse scheme
        let (scheme, rest) = s.split_once(':')?;

        let (body, rest) = rest.split_once('?').unwrap_or((rest, ""));

        let params = rest
            .split('&')
            .filter_map(UriParam::parse)
            .collect::<Vec<_>>();

        Some(Self {
            scheme,
            body: Cow::Borrowed(body),
            params,
        })
    }
}

impl<'a> fmt::Display for Uri<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = self.scheme;
        let body = &self.body;

        write!(f, "{scheme}:{body}")?;

        let mut sep: char = '?';
        for param in &self.params {
            let key = percent_encoding::utf8_percent_encode(
                &param.key,
                &BIP21_ASCII_SET,
            );
            let value = percent_encoding::utf8_percent_encode(
                &param.value,
                &BIP21_ASCII_SET,
            );

            write!(f, "{sep}{key}={value}")?;
            sep = '&';
        }
        Ok(())
    }
}

/// A single `<key>=<value>` URI parameter. The `value` is percent-encoded.
#[derive(Debug)]
struct UriParam<'a> {
    key: Cow<'a, str>,
    value: Cow<'a, str>,
}

impl<'a> UriParam<'a> {
    fn parse(s: &'a str) -> Option<Self> {
        let (key, value) = s.split_once('=')?;
        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8()
            .ok()?;
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        Some(Self { key, value })
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::arbitrary::any_mainnet_address;
    use proptest::{prop_assert_eq, proptest, sample::Index};

    use super::*;

    #[test]
    fn test_bip21_uri_manual() {
        // manual test cases

        // just an address
        assert_eq!(
            Bip21Uri::parse("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"
                    )
                    .unwrap(),
                    amount: None,
                    label: None,
                    message: None,
                }),
                invoice: None,
            }),
        );

        // (proptest regression) funky extra arg
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?foo=%aA"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"
                    )
                    .unwrap(),
                    amount: None,
                    label: None,
                    message: None,
                }),
                invoice: None,
            }),
        );

        // weird mixed case `bitcoin:` scheme
        assert_eq!(
            Bip21Uri::parse(
                "BItCoIn:3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV?amount=23.456"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV"
                    )
                    .unwrap(),
                    amount: Some(Amount::from_sats_u32(23_4560_0000)),
                    label: None,
                    message: None,
                }),
                invoice: None,
            }),
        );

        // all caps QR code style
        assert_eq!(
            Bip21Uri::parse(
                "BITCOIN:BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH?label=Luke%20Jr"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"
                    )
                    .unwrap(),
                    amount: None,
                    label: Some("Luke Jr".to_owned()),
                    message: None,
                }),
                invoice: None,
            }),
        );

        // ignore extra param & duplicate param
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw"
                    )
                    .unwrap(),
                    amount: Some(Amount::from_sats_u32(1)),
                    label: None,
                    message: Some("hello world".to_owned()),
                }),
                invoice: None,
            }),
        );

        // ignore onchain if unrecognized req- param
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&req-foo=bar&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip21Uri {
                onchain: None,
                invoice: None,
            }),
        );
    }

    #[test]
    fn test_bip21_uri_props() {
        proptest!(|(uri: Bip21Uri)| {
            // roundtrip: Bip21Uri -> String -> Bip21Uri
            let actual = Bip21Uri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
        });

        proptest!(|(uri: Bip21Uri, key: String, value: String, param_idx: Index)| {
            // inserting a `req-` param should cause us to skip the onchain method

            let mut uri_raw = uri.to_uri();
            let param_idx = param_idx.index(uri_raw.params.len() + 1);
            let key = format!("req-{key}");
            let param = UriParam { key: key.into(), value: value.into() };
            uri_raw.params.insert(param_idx, param);

            let actual1 = Bip21Uri::parse(&uri_raw.to_string()).unwrap();
            let actual2 = Bip21Uri::parse_uri(uri_raw).unwrap();
            prop_assert_eq!(&actual1, &actual2);
            prop_assert_eq!(None, actual1.onchain);
            prop_assert_eq!(uri.invoice, actual1.invoice);
        });

        proptest!(|(address in any_mainnet_address(), junk: String)| {
            // appending junk after the `<address>?` should be fine
            let uri = Bip21Uri {
                onchain: Some(Onchain { address, amount: None, label: None, message: None }),
                invoice: None,
            };
            let uri_str = uri.to_string();
            let uri_str_with_junk = format!("{uri_str}?{junk}");
            let uri_parsed = Bip21Uri::parse(&uri_str_with_junk).unwrap();

            prop_assert_eq!(
                uri.onchain.unwrap().address,
                uri_parsed.onchain.unwrap().address
            );
        });
    }
}
