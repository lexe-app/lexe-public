//! BIP321 / BIP21 URI parsing and formatting
//!
//! BIP 321 is a URI scheme for Bitcoin payments.
//!
//! + [BIP21 - URI Scheme](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki)
//! + [BIP321 - URI Scheme (draft, replaces BIP 21)](https://github.com/bitcoin/bips/pull/1555/files)

use std::{borrow::Cow, fmt, str::FromStr};

use bitcoin::address::NetworkUnchecked;
#[cfg(test)]
use common::ln::amount;
use common::ln::amount::Amount;
use lexe_api_core::types::{invoice::LxInvoice, offer::LxOffer};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;

use crate::{
    helpers::AddressExt,
    payment_method::{Onchain, PaymentMethod},
    uri::{Uri, UriParam},
};

/// A [BIP321](https://github.com/bitcoin/bips/pull/1555/files) /
/// [BIP21](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki) URI
///
/// Wallets are aligning on BIP321 as the standard to encode not just on-chain
/// payment requests, but also Lightning invoices and offers, slient payments,
/// future bitcoin address types, etc...
///
/// If you want a BIP21 URI with a legacy P2PKH or P2SH address, it must be the
/// first `onchain` address. It will be placed in the URI body.
///
/// Examples:
///
/// ```not_rust
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?label=Luke-Jr
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=20.3&label=Luke-Jr
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?somethingyoudontunderstand=50&somethingelseyoudontget=999
/// ```
#[derive(Debug, Default)]
#[cfg_attr(test, derive(Arbitrary, Eq, PartialEq))]
pub struct Bip321Uri {
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary_impl::arb_bip321_addrs()")
    )]
    pub onchain: Vec<bitcoin::Address<NetworkUnchecked>>,

    // TODO(phlip9): support multiple invoices?
    pub invoice: Option<LxInvoice>,

    // TODO(phlip9): support multiple offers?
    pub offer: Option<LxOffer>,

    /// On-chain amount
    #[cfg_attr(
        test,
        proptest(strategy = "amount::arb::sats_amount().prop_map(Some)")
    )]
    pub amount: Option<Amount>,

    /// On-chain label / vendor
    pub label: Option<String>,

    /// On-chain message / payment note
    pub message: Option<String>,
    //
    // TODO(phlip9): "pop" (proof-of-payment) callback param
}

impl Bip321Uri {
    const URI_SCHEME: &'static str = "bitcoin";

    /// See: [`crate::payment_uri::PaymentUri::any_usable`]
    pub fn any_usable(&self) -> bool {
        !self.onchain.is_empty()
            || self.invoice.is_some()
            || self.offer.is_some()
    }

    pub fn matches_scheme(scheme: &str) -> bool {
        // Use `eq_ignore_ascii_case` as it's technically in-spec for the scheme
        // to be upper, lower, or even mixed case.
        scheme.eq_ignore_ascii_case(Self::URI_SCHEME)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let uri = Uri::parse(s)?;
        Self::parse_uri(uri)
    }

    fn parse_uri(uri: Uri) -> Option<Self> {
        if !Self::matches_scheme(uri.scheme) {
            return None;
        }

        Some(Self::parse_uri_inner(uri))
    }

    pub(crate) fn parse_uri_inner(uri: Uri) -> Self {
        debug_assert!(Self::matches_scheme(uri.scheme));

        let mut out = Self {
            onchain: Vec::new(),
            invoice: None,
            offer: None,
            amount: None,
            label: None,
            message: None,
        };

        // Skip the `Onchain` method if we see any unrecognized `req-`
        // parameters, as per the spec. However, we're going to partially ignore
        // the spec and unconditionally parse out BOLT11 and BOLT12 pieces,
        // since they're fully self-contained formats. This probably won't be an
        // issue regardless, since `req-` params aren't used much in practice.
        let mut skip_onchain = false;

        // Parse "bitcoin:{address}" from URI body
        if let Ok(address) = bitcoin::Address::from_str(&uri.body) {
            out.onchain.push(address);
        }

        // Parse URI parameters
        for param in uri.params {
            use bitcoin::Network;

            let key = param.key_parsed();

            if key.is("lightning") {
                if out.invoice.is_none() {
                    if let Ok(invoice) = LxInvoice::from_str(&param.value) {
                        out.invoice = Some(invoice);
                        continue;
                    }
                }
                if out.offer.is_none() {
                    if let Ok(offer) = LxOffer::from_str(&param.value) {
                        // bitcoinqr.dev showcases an offer inside a
                        // `lightning` parameter
                        out.offer = Some(offer);
                        continue;
                    }
                }
            } else if key.is("lno") || /* legacy */ key.is("b12") {
                if out.offer.is_none() {
                    out.offer = LxOffer::from_str(&param.value).ok();
                }
            } else if key.is("bc") {
                if let Ok(address) = bitcoin::Address::from_str(&param.value) {
                    if address.is_valid_for_network(Network::Bitcoin) {
                        out.onchain.push(address);
                    }
                }
            } else if key.is("tb") {
                if let Ok(address) = bitcoin::Address::from_str(&param.value) {
                    if address.is_valid_for_network(Network::Testnet)
                        || address.is_valid_for_network(Network::Testnet4)
                        || address.is_valid_for_network(Network::Signet)
                    {
                        out.onchain.push(address);
                    }
                }
            } else if key.is("bcrt") {
                if let Ok(address) = bitcoin::Address::from_str(&param.value) {
                    if address.is_valid_for_network(Network::Regtest) {
                        out.onchain.push(address);
                    }
                }
            } else if key.is("amount") {
                if out.amount.is_none() {
                    out.amount = parse_onchain_btc_amount(&param.value);
                }
            } else if key.is("label") {
                if out.label.is_none() {
                    out.label = Some(param.value.into_owned());
                }
            } else if key.is("message") {
                if out.message.is_none() {
                    out.message = Some(param.value.into_owned());
                }
            } else if key.is_req {
                // We'll respect required && unrecognized bip21 params by
                // throwing out all onchain methods.
                skip_onchain = true;
            }

            // ignore duplicates or other keys
        }

        // Throw out all on-chain methods if we see any unrecognized &&
        // required bip21 params.
        if skip_onchain {
            out.onchain = Vec::new();
            out.amount = None;
            out.label = None;
            out.message = None;
        }

        out
    }

    fn to_uri(&self) -> Uri<'_> {
        let mut out = Uri {
            scheme: Self::URI_SCHEME,
            body: Cow::Borrowed(""),
            params: Vec::new(),
        };

        // If the first address is supported in the URI body, use it as the
        // body.
        let onchain = match self.onchain.split_first() {
            Some((address, rest)) if address.is_supported_in_uri_body() => {
                out.body = Cow::Owned(address.assume_checked_ref().to_string());
                rest
            }
            _ => self.onchain.as_slice(),
        };

        // Add all remaining onchain addresses as URI params
        for address in onchain {
            use bitcoin::Network;

            // P2PKH and P2SH addresses don't have an HRP and so can't go in
            // the URI query params.
            if !address.is_supported_in_uri_query_param() {
                debug_assert!(false, "Should have been placed in URI body");
                continue;
            }

            // Get the HRP for this address
            let hrp = if address.is_valid_for_network(Network::Bitcoin) {
                "bc"
            } else if address.is_valid_for_network(Network::Testnet)
                || address.is_valid_for_network(Network::Testnet4)
                || address.is_valid_for_network(Network::Signet)
            {
                "tb"
            } else if address.is_valid_for_network(Network::Regtest) {
                "bcrt"
            } else {
                debug_assert!(false, "Unsupported network");
                continue;
            };

            out.params.push(UriParam {
                key: Cow::Borrowed(hrp),
                value: Cow::Owned(address.assume_checked_ref().to_string()),
            });
        }

        // BIP21 onchain amount
        if let Some(amount) = &self.amount {
            out.params.push(UriParam {
                key: Cow::Borrowed("amount"),
                // We need to round to satoshi-precision for this to be a
                // valid on-chain amount.
                value: Cow::Owned(amount.round_sat().btc().to_string()),
            });
        }

        // BIP21 onchain label
        if let Some(label) = &self.label {
            out.params.push(UriParam {
                key: Cow::Borrowed("label"),
                value: Cow::Borrowed(label),
            });
        }

        // BIP21 onchain message
        if let Some(message) = &self.message {
            out.params.push(UriParam {
                key: Cow::Borrowed("message"),
                value: Cow::Borrowed(message),
            });
        }

        // BOLT11 invoice param
        if let Some(invoice) = &self.invoice {
            out.params.push(UriParam {
                key: Cow::Borrowed("lightning"),
                value: Cow::Owned(invoice.to_string()),
            });
        }

        // BOLT12 offer param
        if let Some(offer) = &self.offer {
            out.params.push(UriParam {
                key: Cow::Borrowed("lno"),
                value: Cow::Owned(offer.to_string()),
            });
        }

        out
    }

    /// "Flatten" the [`Bip321Uri`] into its component [`PaymentMethod`]s.
    pub(crate) fn flatten(self) -> Vec<PaymentMethod> {
        let mut out = Vec::with_capacity(
            self.onchain.len()
                + self.invoice.is_some() as usize
                + self.offer.is_some() as usize,
        );

        for address in self.onchain {
            out.push(PaymentMethod::Onchain(Onchain {
                address,
                amount: self.amount,
                label: self.label.clone(),
                message: self.message.clone(),
            }));
        }

        if let Some(invoice) = self.invoice {
            out.push(PaymentMethod::Invoice(invoice));
        }

        if let Some(offer) = self.offer {
            out.push(PaymentMethod::Offer(offer));
        }

        out
    }
}

impl fmt::Display for Bip321Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_uri(), f)
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use bitcoin::address::NetworkUnchecked;
    use common::test_utils::arbitrary::any_mainnet_addr_unchecked;
    use proptest::strategy::Strategy;

    use crate::helpers::AddressExt;

    // Generate a list of BIP321 address to go in a [`Bip321Uri`]. To support
    // roundtripping, we filter out any P2PKH or P2SH addresses that aren't in
    // the first position.
    pub(crate) fn arb_bip321_addrs(
    ) -> impl Strategy<Value = Vec<bitcoin::Address<NetworkUnchecked>>> {
        proptest::collection::vec(any_mainnet_addr_unchecked(), 0..3).prop_map(
            |addrs| {
                addrs
                    .into_iter()
                    .enumerate()
                    .filter_map(|(idx, addr)| {
                        if idx != 0 && !addr.is_supported_in_uri_query_param() {
                            None
                        } else {
                            Some(addr)
                        }
                    })
                    .collect()
            },
        )
    }
}

/// Parse an onchain amount in BTC, e.g. "1.0024" => 1_0024_0000 sats. This
/// parser also rounds to the nearest satoshi amount, since on-chain
/// payments are limited to satoshi precision.
fn parse_onchain_btc_amount(s: &str) -> Option<Amount> {
    Decimal::from_str(s)
        .ok()
        .and_then(|btc_decimal| Amount::try_from_btc(btc_decimal).ok())
        // On-chain min. denomination
        .map(|amount| amount.round_sat())
}

#[cfg(test)]
mod test {
    use std::{borrow::Cow, str::FromStr};

    use common::{
        ln::amount::Amount, test_utils::arbitrary::any_mainnet_addr_unchecked,
    };
    use lexe_api_core::types::offer::LxOffer;
    use proptest::{prop_assert_eq, proptest};

    use super::*;
    use crate::{payment_uri::PaymentUri, uri::UriParam};

    #[test]
    fn test_bip321_uri_manual() {
        // manual test cases

        // just an address
        let address =
            bitcoin::Address::from_str("13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU")
                .unwrap();
        assert_eq!(
            Bip321Uri::parse("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
            Some(Bip321Uri {
                onchain: vec![address.clone()],
                ..Bip321Uri::default()
            }),
        );

        // (proptest regression) funky extra arg
        assert_eq!(
            Bip321Uri::parse(
                "bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?foo=%aA"
            ),
            Some(Bip321Uri {
                onchain: vec![address.clone()],
                ..Bip321Uri::default()
            }),
        );

        // weird mixed case `bitcoin:` scheme
        assert_eq!(
            Bip321Uri::parse(
                "BItCoIn:3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV?amount=23.456"
            ),
            Some(Bip321Uri {
                onchain: vec![bitcoin::Address::from_str(
                    "3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV"
                )
                .unwrap()],
                amount: Some(Amount::from_sats_u32(23_4560_0000)),
                ..Bip321Uri::default()
            }),
        );

        // all caps QR code style
        assert_eq!(
            Bip321Uri::parse(
                "BITCOIN:BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH?label=Luke%20Jr"
            ),
            Some(Bip321Uri {
                onchain: vec![
                    bitcoin::Address::from_str("bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh").unwrap(),
                ],
                label: Some("Luke Jr".to_owned()),
                ..Bip321Uri::default()
            }),
        );

        // ignore extra param & duplicate param
        assert_eq!(
            Bip321Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip321Uri {
                onchain: vec![
                    bitcoin::Address::from_str("bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw").unwrap(),
                ],
                amount: Some(Amount::from_sats_u32(1)),
                message: Some("hello world".to_owned()),
                ..Bip321Uri::default()
            }),
        );

        // ignore onchain if unrecognized req- param
        assert_eq!(
            Bip321Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&req-foo=bar&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip321Uri::default()),
        );

        // BOLT12 offer
        let address_str =
            "bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw";
        let address = bitcoin::Address::from_str(address_str).unwrap();
        let offer_str =
            "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";
        let offer = LxOffer::from_str(offer_str).unwrap();
        let expected = Some(Bip321Uri {
            onchain: vec![address.clone()],
            offer: Some(offer.clone()),
            ..Bip321Uri::default()
        });
        // Support both `lightning=<offer>` and `lno=<offer>` params.
        let actual1 =
            Bip321Uri::parse(&format!("bitcoin:{address_str}?lno={offer_str}"));
        let actual2 = Bip321Uri::parse(&format!(
            "bitcoin:{address_str}?lightning={offer_str}"
        ));
        assert_eq!(actual1, expected);
        assert_eq!(actual2, expected);
    }

    // roundtrip: Bip321Uri -> String -> Bip321Uri
    #[test]
    fn test_bip321_uri_prop_roundtrip() {
        proptest!(|(uri: Bip321Uri)| {
            let actual = Bip321Uri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
        });
    }

    // appending junk after the `<address>?` should be fine
    #[test]
    fn test_bip321_uri_prop_append_junk() {
        proptest!(|(address in any_mainnet_addr_unchecked(), junk: String)| {
            let uri = Bip321Uri {
                onchain: vec![address],
                ..Bip321Uri::default()
            };
            let uri_str = uri.to_string();
            let uri_str_with_junk = format!("{uri_str}?{junk}");
            let uri_parsed = Bip321Uri::parse(&uri_str_with_junk).unwrap();

            prop_assert_eq!(
                uri.onchain.first().unwrap(),
                uri_parsed.onchain.first().unwrap()
            );
        });
    }

    // support `lightning=<offer>` param
    #[test]
    fn test_bip321_uri_prop_lightning_offer_param() {
        proptest!(|(uri: Bip321Uri, offer: LxOffer)| {
            let mut uri_raw = uri.to_uri();
            let offer_str = Cow::Owned(offer.to_string());
            let param = UriParam { key: "lightning".into(), value: offer_str };
            uri_raw.params.insert(0, param);

            let actual = Bip321Uri::parse_uri(uri_raw).unwrap();
            let mut expected = uri;
            expected.offer = Some(offer);

            prop_assert_eq!(actual, expected);
        });
    }

    #[rustfmt::skip] // Stop breaking comments
    #[test]
    fn test_bip321_test_vectors() {
        // Must parse and roundtrip
        #[track_caller]
        fn parse_ok_rt(s: &str) -> PaymentUri {
            let uri = PaymentUri::parse(s).unwrap();
            // Ensure it roundtrips
            assert_eq!(s, uri.to_string());
            uri
        }

        // It'll at least parse with some usable `PaymentMethod`s
        #[track_caller]
        fn parse_ok(s: &str) -> PaymentUri {
            let uri = PaymentUri::parse(s).unwrap();
            assert!(uri.any_usable());
            uri
        }

        // Parses but no usable `PaymentMethod`
        #[track_caller]
        fn parse_ok_unusable(s: &str) {
            let uri = PaymentUri::parse(s).unwrap();
            assert!(!uri.any_usable());
        }

        // NOTE: these test vectors are edited to use valid
        // addresses/invoices/offers/etc, otherwise we don't parse them.

        // basic, well-formed URIs that we can fully roundtrip
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?label=Luke-Jr");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?label=Luke-Jr");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek");
        parse_ok_rt("bitcoin:?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek");
        parse_ok_rt("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q");

        assert_eq!(
            parse_ok("bitcoin:?bc=bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw&bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("bitcoin:?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
            parse_ok("bitcoin:bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("bitcoin:?tb=tb1qkkxnp5zm6wpfyjufdznh38vm03u4w8q8awuggp"),
            parse_ok("bitcoin:tb1qkkxnp5zm6wpfyjufdznh38vm03u4w8q8awuggp"),
        );
        assert_eq!(
            parse_ok("bitcoin:?bcrt=bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"),
            parse_ok("bitcoin:bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"),
        );

        // TODO(phlip9): why does decimal amount 20.3 - roundtrip -> 20.30
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=20.3&label=Luke-Jr"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=20.30&label=Luke-Jr"),
        );

        // TODO(phlip9): "parse" silent payments
        assert_eq!(
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q&sp=sp1qsilentpayment"),
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q"),
        );
        parse_ok_unusable("bitcoin:?sp=sp1qsilentpayment");
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?sp=sp1qsilentpayment"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
        );

        // we currently normalize to lowercase
        assert_eq!(
            parse_ok("BITCOIN:BC1QM9R9X9H2C9WPTAZ0873VYFV8CKX2LCDX8F48UCTTZQFT7R0Q2YASXKT2LW?BC=BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("BITCOIN:?BC=BC1QM9R9X9H2C9WPTAZ0873VYFV8CKX2LCDX8F48UCTTZQFT7R0Q2YASXKT2LW&BC=BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );

        // ignore unrecognized, not-required params
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?somethingyoudontunderstand=50&somethingelseyoudontget=999"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
        );

        // unrecognized req- params => whole on-chain method is unusable
        parse_ok_unusable("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999");
        // but still parse out offers/invoices
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q&somethingelseyoudontget=999"),
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q"),
        );
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek&somethingelseyoudontget=999"),
            parse_ok("bitcoin:?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek"),
        );
    }
}
