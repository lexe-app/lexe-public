//! Permissive decoding of bitcoin+lightning payment addresses+URIs.
//!
//! This module parses various BTC-related payment methods permissively. That
//! means we should parse inputs that are not strictly well-formed.
//!
//! Standards:
//! + [BIP21 - URI Scheme](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki)
//! + [BIP321 - URI Scheme (draft, replaces BIP 21)](https://github.com/bitcoin/bips/pull/1555/files)
//!
//! Other wallet parsers for comparison:
//! + [ACINQ/phoenix - Parser](https://github.com/ACINQ/phoenix/blob/master/phoenix-shared/src/commonMain/kotlin/fr.acinq.phoenix/utils/Parser.kt)
//! + [breez/breez-sdk - input_parser.rs](https://github.com/breez/breez-sdk-greenlight/blob/main/libs/sdk-common/src/input_parser.rs)
//! + [MutinyWallet/bitcoin_waila (unmaintained)](https://github.com/MutinyWallet/bitcoin-waila/blob/master/waila/src/lib.rs)

// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]

use std::{
    borrow::Cow,
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::ensure;
use bitcoin::address::{NetworkUnchecked, NetworkValidation};
use common::ln::{amount::Amount, network::LxNetwork};
#[cfg(test)]
use common::{ln::amount, test_utils::arbitrary};
use lexe_api_core::types::{
    invoice::{self, LxInvoice},
    offer::{self, LxOffer},
};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;

/// Refuse to parse any input longer than this many KiB.
const MAX_LEN_KIB: usize = 8;

/// A decoded "Payment URI", usually from a scanned QR code, manually pasted
/// code, or handling a URI open (like tapping a `bitcoin:bc1qfjeyfl...` URI in
/// your mobile browser or in another app).
///
/// Many variants give multiple ways to pay, with e.g. BOLT11 invoices including
/// an onchain fallback, or BIP321 URIs including an optional BOLT11 invoice.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum PaymentUri {
    /// An BIP321 URI, containing an onchain payment description, plus optional
    /// BOLT11 invoice and/or BOLT12 offer.
    ///
    /// ex: "bitcoin:bc1qfj..."
    ///     "bitcoin:?lno=lno1pqps7..."
    Bip321Uri(Bip321Uri),

    /// A Lightning URI, containing a BOLT11 invoice or BOLT12 offer.
    ///
    /// ex: "lightning:lnbc1pvjlue..." or
    ///     "lightning:lno1pqps7..."
    LightningUri(LightningUri),

    /// A standalone BOLT11 Lightning invoice.
    ///
    /// ex: "lnbc1pvjlue..."
    Invoice(LxInvoice),

    /// A standalone BOLT12 Lightning offer.
    ///
    /// ex: "lno1pqps7sj..."
    // TODO(phlip9): BOLT12 refund
    Offer(LxOffer),

    /// A standalone onchain Bitcoin address.
    ///
    /// ex: "bc1qfjeyfl..."
    #[cfg_attr(
        test,
        proptest(
            strategy = "arbitrary::any_mainnet_addr_unchecked().prop_map(Self::Address)"
        )
    )]
    Address(bitcoin::Address<NetworkUnchecked>),
    //
    // Bip353Address(Bip353Address),
    // EmailLookingAddress(EmailLookingAddress),
    // Lnurl(Lnurl),
    //
    //
    // NOTE: adding support for a new URI scheme? Remember to add it in these
    // places!
    //
    // app/ios/Runner/Info.plist
    // app/macos/Runner/Info.plist
    // app/android/app/src/main/AndroidManifest.xml
}

impl PaymentUri {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        // Refuse to parse anything longer than `MAX_LEN_KIB` KiB
        if s.len() > (MAX_LEN_KIB << 10) {
            return Err(ParseError::TooLong);
        }

        let s = s.trim();

        // Try parsing a URI-looking thing
        //
        // ex: "bitcoin:bc1qfj..." or
        //     "lightning:lnbc1pvjlue..." or
        //     "lightning:lno1pqps7..." or ...
        if let Some(uri) = Uri::parse(s) {
            // ex: "bitcoin:bc1qfj..."
            if Bip321Uri::matches_scheme(uri.scheme) {
                return Ok(Self::Bip321Uri(Bip321Uri::parse_uri_inner(uri)));
            }

            // ex: "lightning:lnbc1pvjlue..." or
            //     "lightning:lno1pqps7..."
            if LightningUri::matches_scheme(uri.scheme) {
                return Ok(Self::LightningUri(LightningUri::parse_uri_inner(
                    uri,
                )));
            }

            if Lnurl::matches_scheme(uri.scheme) {
                return Err(ParseError::LnurlUnsupported);
            }

            return Err(ParseError::BadScheme);
        }

        // TODO(phlip9): support BIP353
        // TODO(phlip9): phoenix parser also attempts to strip "%E2%82%BF"
        //               %-encoded BTC symbol.
        // The unicode here is the B bitcoin currency symbol.
        // ex: "₿philip@lexe.app"
        if let Some(_hrn) = Bip353Address::matches(s) {
            return Err(ParseError::Bip353Unsupported);
        }

        // TODO(phlip9): support BIP353 / Lightning Address
        // ex: "philip@lexe.app"
        if let Some((_local, _domain)) = EmailLookingAddress::matches(s) {
            return Err(ParseError::EmailLookingUnsupported);
        }

        // ex: "lnbc1pvjlue..."
        if LxInvoice::matches_hrp_prefix(s) {
            return LxInvoice::from_str(s)
                .map(Self::Invoice)
                .map_err(ParseError::InvalidInvoice);
        }

        // ex: "lno1pqps7sj..."
        if LxOffer::matches_hrp_prefix(s) {
            return LxOffer::from_str(s)
                .map(Self::Offer)
                .map_err(ParseError::InvalidOffer);
        }

        // ex: "lnurl1dp68g..."
        if Lnurl::matches_hrp_prefix(s) {
            return Err(ParseError::LnurlUnsupported);
        }

        // ex: "bc1qfjeyfl..."
        if bitcoin::Address::matches_hrp_prefix(s) {
            return bitcoin::Address::from_str(s)
                .map(Self::Address)
                .map_err(ParseError::InvalidBtcAddress);
        }
        // The block above only handles modern bech32 segwit+taproot addresses.
        // We don't have a good way to know ahead of time if this is a legacy
        // bitcoin address or not, so we just have to try but throw away the
        // error.
        if let Ok(address) = bitcoin::Address::from_str(s) {
            return Ok(Self::Address(address));
        }

        Err(ParseError::UnknownCode)
    }

    /// "Flatten" the [`PaymentUri`] into its component [`PaymentMethod`]s.
    pub fn flatten(self) -> Vec<PaymentMethod> {
        match self {
            Self::Bip321Uri(bip321) => bip321.flatten(),
            Self::LightningUri(lnuri) => lnuri.flatten(),
            Self::Invoice(invoice) => {
                let mut out = Vec::with_capacity(1);
                flatten_invoice_into(invoice, &mut out);
                out
            }
            Self::Offer(offer) => vec![PaymentMethod::Offer(offer)],
            Self::Address(address) =>
                vec![PaymentMethod::Onchain(Onchain::from(address))],
        }
    }

    /// Resolve the `PaymentUri` into a single, "best" [`PaymentMethod`].
    //
    // phlip9: this impl is currently pretty dumb and just unconditionally
    // returns the first (valid) BOLT11 invoice it finds, o/w onchain. It's not
    // hard to imagine a better strategy, like using our current
    // liquidity/balance to decide onchain vs LN, or returning all methods and
    // giving the user a choice. This'll also need to be async in the future, as
    // we'll need to fetch invoices from any LNURL endpoints we come across.
    pub fn resolve_best(
        self,
        network: LxNetwork,
    ) -> anyhow::Result<PaymentMethod> {
        // A single scanned/opened PaymentUri can contain multiple different
        // payment methods (e.g., a LN BOLT11 invoice + an onchain fallback
        // address).
        let mut payment_methods = self.flatten();

        // Filter out all methods that aren't valid for our current network
        // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
        payment_methods.retain(|method| method.supports_network(network));
        ensure!(
            !payment_methods.is_empty(),
            "Payment code is not valid for {network}"
        );

        // Pick the most preferable payment method.
        let best = payment_methods
            .into_iter()
            .max_by_key(|x| match x {
                PaymentMethod::Invoice(_) => 20,
                PaymentMethod::Onchain(o) => 10 + o.relative_priority(),
                // TODO(phlip9): increase priority when BOLT12 support
                PaymentMethod::Offer(_) => {
                    debug_assert!(false, "BOLT12 not supported yet");
                    0
                }
            })
            .expect("We just checked there's at least one method");

        // TODO(phlip9): remove when BOLT12 support
        ensure!(
            !best.is_offer(),
            "Lexe doesn't currently support Lightning BOLT12 Offers",
        );

        Ok(best)
    }

    /// Returns true if there are any usable [`PaymentMethod`]s in this URI.
    ///
    /// This method is equivalent to `!self.flatten().is_empty()`, but doesn't
    /// require consuming the `PaymentUri` and flattening.
    pub fn any_usable(&self) -> bool {
        match self {
            Self::Bip321Uri(uri) => uri.any_usable(),
            Self::LightningUri(uri) => uri.any_usable(),
            Self::Invoice(_) => true,
            Self::Offer(_) => true,
            Self::Address(_) => true,
        }
    }
}

impl fmt::Display for PaymentUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::fmt::Display;
        match self {
            Self::Address(address) =>
                Display::fmt(&address.assume_checked_ref(), f),
            Self::Invoice(invoice) => Display::fmt(invoice, f),
            Self::Offer(offer) => Display::fmt(offer, f),
            Self::LightningUri(ln_uri) => Display::fmt(ln_uri, f),
            Self::Bip321Uri(bip321_uri) => Display::fmt(bip321_uri, f),
        }
    }
}

/// "Flatten" an [`LxInvoice`] into its "component" [`PaymentMethod`]s, pushing
/// them into an existing `Vec`.
fn flatten_invoice_into(invoice: LxInvoice, out: &mut Vec<PaymentMethod>) {
    let onchain_fallback_addrs = invoice.onchain_fallbacks();
    out.reserve(1 + onchain_fallback_addrs.len());

    // BOLT11 invoices may include onchain fallback addresses.
    if !onchain_fallback_addrs.is_empty() {
        let description = invoice.description_str().map(str::to_owned);
        let amount = invoice.amount();

        for addr in onchain_fallback_addrs {
            // TODO(max): Upstream an `Address::into_unchecked` to avoid clone
            let address = addr.as_unchecked().clone();
            out.push(PaymentMethod::Onchain(Onchain {
                address,
                amount,
                label: None,
                message: description.clone(),
            }));
        }
    }

    out.push(PaymentMethod::Invoice(invoice));
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    TooLong,
    BadScheme,
    UnknownCode,
    EmailLookingUnsupported,
    Bip353Unsupported,
    LnurlUnsupported,
    InvalidInvoice(invoice::ParseError),
    InvalidOffer(offer::ParseError),
    InvalidBtcAddress(bitcoin::address::ParseError),
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "Payment code is too long to parse (>{MAX_LEN_KIB} KiB)"
            ),
            Self::BadScheme => write!(f, "Unrecognized payment URI scheme"),
            Self::UnknownCode => write!(f, "Unrecognized payment code"),
            Self::EmailLookingUnsupported => write!(
                f,
                "Lightning Addresses and BIP353 are not supported yet"
            ),
            Self::Bip353Unsupported => write!(f, "BIP353 is not supported yet"),
            Self::LnurlUnsupported => write!(f, "LNURL is not supported yet"),
            Self::InvalidInvoice(err) => Display::fmt(err, f),
            Self::InvalidOffer(err) => Display::fmt(err, f),
            Self::InvalidBtcAddress(err) =>
                write!(f, "Failed to parse on-chain address: {err}"),
        }
    }
}

/// A single "payment method" -- each kind here should correspond with a single
/// linear payment flow for a user, where there are no other alternate methods.
///
/// For example, a Unified BTC QR code contains a single [`Bip321Uri`], which
/// may contain _multiple_ discrete payment methods (an onchain address, a
/// BOLT11 invoice, a BOLT12 offer).
#[allow(clippy::large_enum_variant)]
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(LxInvoice),
    Offer(LxOffer),
}

impl PaymentMethod {
    pub fn is_onchain(&self) -> bool {
        matches!(self, Self::Onchain(_))
    }

    pub fn is_invoice(&self) -> bool {
        matches!(self, Self::Invoice(_))
    }

    pub fn is_offer(&self) -> bool {
        matches!(self, Self::Offer(_))
    }

    pub fn supports_network(&self, network: LxNetwork) -> bool {
        match self {
            Self::Onchain(x) => x.supports_network(network),
            Self::Invoice(x) => x.supports_network(network),
            Self::Offer(x) => x.supports_network(network),
        }
    }
}

/// An onchain payment method, usually parsed from a standalone BTC address or
/// BIP321 URI.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Onchain {
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_mainnet_addr_unchecked()")
    )]
    pub address: bitcoin::Address<NetworkUnchecked>,

    #[cfg_attr(
        test,
        proptest(strategy = "amount::arb::sats_amount().prop_map(Some)")
    )]
    pub amount: Option<Amount>,

    /// The recipient/payee name.
    pub label: Option<String>,

    /// The payment description.
    pub message: Option<String>,
}

impl Onchain {
    #[inline]
    pub fn supports_network(&self, network: LxNetwork) -> bool {
        self.address.is_valid_for_network(network.to_bitcoin())
    }

    /// Returns the relative priority for this onchain address. Higher = better.
    fn relative_priority(&self) -> usize {
        use bitcoin::AddressType::*;
        let address_type =
            match self.address.assume_checked_ref().address_type() {
                Some(x) => x,
                // Non-standard
                None => return 0,
            };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => 2,
            // Pay to script hash.
            P2sh => 2,
            // Pay to witness pubkey hash.
            P2wpkh => 4,
            // Pay to witness script hash.
            P2wsh => 4,
            // Pay to taproot.
            // TODO(phlip9): can we pay to taproot yet?
            P2tr => 3,
            // Unknown standard
            _ => 1,
        }
    }
}

impl<V: NetworkValidation> From<bitcoin::Address<V>> for Onchain {
    fn from(addr: bitcoin::Address<V>) -> Self {
        // TODO(max): Upstream an `Address::into_unchecked` to avoid clone
        let address = addr.as_unchecked().clone();
        Self {
            address,
            amount: None,
            label: None,
            message: None,
        }
    }
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
#[derive(Debug, Default, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Bip321Uri {
    #[cfg_attr(test, proptest(strategy = "test::arb_bip321_addrs()"))]
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

    /// See: [`PaymentUri::any_usable`]
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

    fn parse_uri_inner(uri: Uri) -> Self {
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
    fn flatten(self) -> Vec<PaymentMethod> {
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

/// A "lightning:" URI, containing a BOLT11 invoice or BOLT12 offer.
///
/// Examples:
///
/// ```not_rust
/// // Short bolt11 invoice
/// lightning:lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek
///
/// // Long bolt11 invoice
/// lightning:lnbc1sd2trl2dk89neerzdaa7amlu9knxnn4malh5jqpmamhle2tr5dym3gpth0h777lwalt5j099a3nr3gpthjk7mm5tp2vg7qnuy7n2gh70l3k27e3s49fmhml0gmc24l9r4xhrcfl9d5vqqqkf32yncmafyca7lm64hjkj76mu5khxk0fp5khnctfpay7x0g88ez8umjv6734rp2tudu3wajgfl0hwll92u3slcfl9d5y3xlr8arhfyjjcmf7z5f82gt7x8trvja003gpt30xuqz5hxghfg0r2us4mcelrulc2alp8u4k32r5gqm7xgmmtgqxqawlwal7wle8ysm7zcg30tn0asv9fg2p9wr6zmq9snv8wpy0dlr4ud4jzeksz0n5x76yaml72h6fdxz5h5fdye07z0etd8shxufjq83nz7ez003jzem2zfza7am7hp2ygnhluyljk60pfa9nsdj9s542ydn5ap0c2208fdtkevsqufzc2jhyzhsn75t0udk56tn6r03jjcgms5m9meg9fycdrjfttqm9cawlwalyle6a25xfplwlwalyheel2vmgumar283s2427cp67wwtpf3afqhq6sjucw78ktsd85xu8rgqyfhqqelzszh9jq8s5x5fuxe07xc6nzg2u2q2uze9yt3gptkghtcfl9d5wul4q23ly32hkrg2dyylr09wnte20wvr72vgh2gpp5q743y4905ggtpapzar8zv3xrkwgfpf7ncq2250kyjgdtxwl7gukqsp53ep2hsj7v0wqy4955x4gfd76qtmehqahledhhey3pjr5ujk4252s9q2sqqqqqysgqcqyp9pknp4qtyrfeasgkr47x72mr6lk0j4665lt4sagdtqark9f3scnymy8cj6zfp4qlwkynsvgrlmy6nkpgv406jccg8fxullw3h9tcqxv5umtx58s20yqr9yq0pf60wnpe294v93fxngec4djvzkqmwu8vpkwecjd7x3xtur8qc4t7dla5tjnkj5f9jnyc5zspavyj6zkqp6n5hpzx8qu69meek3my06q7lznjlhtz6m73ag6lhm3x5t5x0xu2atmnpqnpsefcqedcc8lfz2l02rrsrqmrssz3yqhvejp8r9qpnhea58q8mk890e062tzf9dc5p2sjp56u44zynzdevqfwxxu07kw93g4ex7p2ujd246syj5nlpv5zgv5rfepxygr3rugx2wjm6tgjzm6tu0pv842ajm8uejknh0xlytt95luge0g53ww4y7dzqzcty6fl02cpl4le2ydqdertw2c5g9k664u0revt9h6du0fv6wrszt0qq94kjsw
///
/// // Short bolt12 offer
/// lightning:lno1pgqpvggzdug5w9m7sr4qdrdynkw90mmm2g9qc93ulym3w8pfzc2xmkpdt6es
///
/// // Long bolt12 offer
/// lightning:lno1pt7srytwezar7l6w2ulj4uv932cu3w327zhmmys2fcqzvt6t9rct4d48tfy098avnujrcfx34rcm3y4pyhec8yyw9lcel8ydyhpt2g0352nf8m5vh8efp09nvf4y7agt726ft26u9megd0u37x6eh9n572464ynm7j9f9xln37uteayx37psmuyn3knj55f69a2rluyljk60r9dd3yer628n3kq2es49724g928zszhrc4mqc2zl8rv53ps3kleh6x50rdaxj3wzjf8nszr2z7734q9nv0wz4vq0p8u4knp2tuvchw50r85ls4zqqz0nkjng3u9usxuk3malhheepvu2xs40fz45hmeemy9t03nl9gyt3ynspsuau2q2u0gd7jqthytq9c94euv6h2njeuyljk6rluyljk683s497zmgtfcqu2cgduyljk6q6tl53zw6au484j5jlu90kw4dr2p9c2jnlc5q4mp222nq7wyfnt2z9qjwl0aapdw0p2vrnnceffagu2q2ac5q4mee4tvf84z8kf6x7xu6rwl34x5c5apw6x5qkempvnhmh0l3hz024s469gn0rw485cn0fq4usaw0p0uzjfvz2flnkk3m7lln5z8m22hs4x4tzukrkthmh0cqpcygqjlqvl3l000tzr7szgsrcc8my32l79fckr7nj63n4f4alqznxem3w0l2er4nd4q9mp7fkuysxv5mxwf0gkjawaz4kxdgdp0dt8uncu9xtmuwl6mrjrnh5jg0tgpdqvpmnqytlhs98r8xynazuqqf3w0f4ps7mchujfkn38nps73dcvrxxuqqxdu0w6z9uplpajgz6ujphevcrccrzav2euc7h9ydctaeg9j8es0e3an5c9js5vmqp6ukr6326ae5e0avhxjs82pepypgz5edrrpjgfhxhccmtq0nuljxdcjr5lxeyg9vqkgtr3e5qqejl7s98yzewt6avqfpscw7chgyty43cmp0vml2qm7uk6gg96tglm2ucqfwjkjsd5rd6smc45gtr3n87p9qxq5era4fcqty72z984af4cgw9pzr8f59908hntawk89k0695ycywtsqpqnfufqzscy7eg3rpgvvuwa0vgfgjl5q6u2sm72hch2eu7xu6t08j5j7cpuvyszuk9uu7hxflfpdds0ef384p7zw6dr367xxmdg8352hev2gdc2jlpwywhgav3wsmu2q2uhnx0qdhzghjkw7e2h8n52f2zwnm72qep9e0tysjutnl7wxmhw0nh6pcurwghg4rfuu5379zdu5ajja4xzfuf0cfl9d5p8p2tuyljk6r3s4yp5aqkwkghtceeqy6u2q2ugf00vh5sr00h77j25x34reehxdjc2jhlj969tcfl9d5u2q2as59a7amluujh7qxnuv24xprmuyljk69crw34rptwth3j6jftuyn3wxlpv5ak9au9f0jn6t2au5m3zgkpu49k7nphu4tnkc09r5u5t32dwpy9tnc9pkghfv4kxextte2nzvzvle6p0yttuh9duy5s2vl9tdkknema09wp8p22wkzhynv9ffucwdn7z0jh7ymhs49x4w08294s0ncmvfx7zsg9xcm9t3gptsmytege9dgytce3w4g9g3v9f0r4j73khzu7wrta0g2a7lmmu4xjk9u9tzs2u70rtv3hqmxsleedagpkfcq9mcfl9d56x50p8u4k33n4udtjk97l0aa7ztc82ef9lef4raj7x5m0fc2pfw08za9kyk0r9dn3yhwpg4r727edv7r5chq62fuptccdv5wrva8k93pqd56rmfv68psmvwzzhse6pj4xg7gu6jrzega4qvzs9f9f4wjvftxj
/// ```
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct LightningUri {
    pub invoice: Option<LxInvoice>,
    pub offer: Option<LxOffer>,
}

impl LightningUri {
    const URI_SCHEME: &'static str = "lightning";

    /// See: [`PaymentUri::any_usable`]
    pub fn any_usable(&self) -> bool {
        self.invoice.is_some() || self.offer.is_some()
    }

    fn matches_scheme(scheme: &str) -> bool {
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

    fn parse_uri_inner(uri: Uri) -> Self {
        debug_assert!(Self::matches_scheme(uri.scheme));

        let mut out = LightningUri {
            invoice: None,
            offer: None,
        };

        // Try parsing the body as an invoice or offer

        if let Ok(invoice) = LxInvoice::from_str(&uri.body) {
            out.invoice = Some(invoice);
        } else if let Ok(offer) = LxOffer::from_str(&uri.body) {
            // non-standard
            out.offer = Some(offer);
        }

        // Try parsing from the query params

        for param in uri.params {
            let key = param.key_parsed();

            if key.is("lightning") && out.invoice.is_none() {
                // non-standard
                out.invoice = LxInvoice::from_str(&param.value).ok();
            } else if (key.is("lno") || key.is("b12")) && out.offer.is_none() {
                // non-standard
                out.offer = LxOffer::from_str(&param.value).ok();
            }
            // ignore duplicates or other keys
        }

        out
    }

    fn to_uri(&self) -> Uri<'_> {
        let mut out = Uri {
            scheme: Self::URI_SCHEME,
            body: Cow::Borrowed(""),
            params: Vec::new(),
        };

        // For now, we'll prioritize BOLT11 invoice in the body position, for
        // compatibility.

        if let Some(invoice) = &self.invoice {
            out.body = Cow::Owned(invoice.to_string());

            // If we also have an offer, put it in the "lno" param I guess.
            if let Some(offer) = &self.offer {
                out.params.push(UriParam {
                    key: Cow::Borrowed("lno"),
                    value: Cow::Owned(offer.to_string()),
                });
            }
        } else if let Some(offer) = &self.offer {
            // If we just have an offer, put it in the body
            out.body = Cow::Owned(offer.to_string());
        }

        out
    }

    fn flatten(self) -> Vec<PaymentMethod> {
        let mut out = Vec::with_capacity(
            self.invoice.is_some() as usize + self.offer.is_some() as usize,
        );

        if let Some(invoice) = self.invoice {
            flatten_invoice_into(invoice, &mut out);
        }

        if let Some(offer) = self.offer {
            out.push(PaymentMethod::Offer(offer));
        }

        out
    }
}

impl fmt::Display for LightningUri {
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
    /// These are the ASCII characters that we will percent-encode inside a URI
    /// query string key or value. We're somewhat conservative here and require
    /// all non-alphanumeric characters to be percent-encoded (with the
    /// exception of of a few control characters, designated in [RFC 3986]).
    ///
    /// Only used for encoding. We will decode all percent-encoded characters.
    ///
    /// [RFC 3986]: https://datatracker.ietf.org/doc/html/rfc3986#section-2.3
    const PERCENT_ENCODE_ASCII_SET: percent_encoding::AsciiSet =
        percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');

    // syntax: `<scheme>:<body>?<key1>=<value1>&<key2>=<value2>&...`
    fn parse(s: &'a str) -> Option<Self> {
        // parse scheme
        // ex: "bitcoin:bc1qfj..." -> `scheme = "bitcoin"`
        let (scheme, rest) = s.split_once(':')?;

        // heuristic: limit scheme to 12 characters. If an input exceeds this,
        // then it's probably not a URI.
        if scheme.len() > 12 {
            return None;
        }

        // ex: "bitcoin:bc1qfj...?message=hello" -> `body = "bc1qfj..."`
        let (body, rest) = rest.split_once('?').unwrap_or((rest, ""));

        // ex: "bitcoin:bc1qfj...?message=hello%20world&amount=0.1"
        //     -> `params = [("message", "hello world"), ("amount", "0.1")]`
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

// "{scheme}:{body}?{key1}={value1}&{key2}={value2}&..."
impl fmt::Display for Uri<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = self.scheme;
        let body = &self.body;

        write!(f, "{scheme}:{body}")?;

        let mut sep: char = '?';
        for param in &self.params {
            write!(f, "{sep}{param}")?;
            sep = '&';
        }
        Ok(())
    }
}

/// A single `<key>=<value>` URI parameter.
///
/// + Both `key` and `value` are percent-encoded when displayed.
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

    fn key_parsed(&'a self) -> UriParamKey<'a> {
        UriParamKey::parse(&self.key)
    }
}

// "{key}={value}"
impl fmt::Display for UriParam<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = percent_encoding::utf8_percent_encode(
            &self.key,
            &Uri::PERCENT_ENCODE_ASCII_SET,
        );
        let value = percent_encoding::utf8_percent_encode(
            &self.value,
            &Uri::PERCENT_ENCODE_ASCII_SET,
        );
        write!(f, "{key}={value}")
    }
}

/// Parsed key from a URI "{key}={value}" parameter.
struct UriParamKey<'a> {
    /// The key name. This is case-insensitive.
    ///
    /// ex:     "amount" -> `name = "amount"`
    /// ex:     "AmOuNt" -> `name = "AmOuNt"`
    /// ex: "req-amount" -> `name = "amount"`
    /// ex: "REQ-AMOUNT" -> `name = "AMOUNT"`
    name: &'a str,
    /// Whether this key is a required parameter. Required parameters are
    /// prefixed by "req-" (potentially mixed case).
    is_req: bool,
}

impl<'a> UriParamKey<'a> {
    fn parse(key: &'a str) -> Self {
        match key.split_at_checked(4) {
            Some((prefix, rest)) if prefix.eq_ignore_ascii_case("req-") =>
                Self {
                    name: rest,
                    is_req: true,
                },
            _ => Self {
                name: key,
                is_req: false,
            },
        }
    }

    fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

// TODO(phlip9): support BIP353
// TODO(phlip9): punycode decode?
struct Bip353Address<'a> {
    _user: &'a str,
    _domain: &'a str,
}

impl Bip353Address<'_> {
    fn matches(s: &str) -> Option<&str> {
        s.strip_prefix("₿")
    }

    // TODO(phlip9): support BIP353
    // fn parse(s: &'a str) -> Option<Self> {
    //     Self::parse_inner(Self::matches(s)?)
    // }
    //
    // fn parse_inner(hrn: &'a str) -> Option<Self> {
    //     let (user, domain) = hrn.split_once('@')?;
    //
    //     if user.is_empty() || domain.is_empty() {
    //         return None;
    //     }
    //
    //     // user:
    //     // - DNS name segment
    //     if !is_dns_name_segment(user.as_bytes()) {
    //         return None;
    //     }
    //
    //     // domain:
    //     // - partial DNS name
    //     if !is_dns_name(domain.as_bytes()) {
    //         return None;
    //     }
    //
    //     let dns = format!("{user}.user._bitcoin-payment.{domain}.");
    //     if !is_dns_name(&dns.as_bytes()) {
    //         return None;
    //     }
    //
    //     Some(Self {
    //         _user: user,
    //         _domain: domain,
    //     })
    // }
}

// TODO(phlip9): support BIP353
// fn is_dns_name(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 255
//         && s.split(|&b| b == b'.').all(is_dns_name_segment)
//         && !s.ends_with(&[b'.'])
// }
//
// fn is_dns_name_segment(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 63
//         && s.iter()
//             .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
// }

/// Indeterminate email-looking human-readable payment address.
/// Either BIP353 without the BTC currency prefix or Lightning Address.
struct EmailLookingAddress<'a> {
    _local: &'a str,
    _domain: &'a str,
}

impl EmailLookingAddress<'_> {
    fn matches(s: &str) -> Option<(&str, &str)> {
        s.split_once('@')
    }

    // TODO(phlip9): support BIP353 / Lightning Address
    // fn parse(s: &'a str) -> Option<Self> {
    //     let (local, domain) = Self::matches(s)?;
    //     Self::parse_inner(local, domain)
    // }
    //
    // fn parse_inner(local: &'a str, domain: &'a str) -> Option<Self> {
    //     if local.is_empty() || domain.is_empty() {
    //         return None;
    //     }
    //
    //     // local:
    //     // - Lightning Address: a-z0-9-_.
    //     // - BIP353: DNS name segment
    //     //
    //     // Union:
    //     if !local.as_bytes().iter().all(|&b| {
    //         b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_'
    //     }) {
    //         return None;
    //     }
    //
    //     // domain:
    //     // - Lightning Address: full DNS hostname
    //     // - BIP353: partial DNS name
    //     //
    //     // Union:
    //     if !is_dns_name(domain.as_bytes()) {
    //         return None;
    //     }
    //
    //     Some(Self {
    //         _local: local,
    //         _domain: domain,
    //     })
    // }
}

// TODO(phlip9): support BIP353 / Lightning Address
// /// Returns `true` if `s` is a valid DNS hostname.
// ///
// /// - a-z0-9-. (case insensitive)
// /// - no leading/trailing hyphens
// /// - no empty labels
// /// - no empty domain
// /// - no more than 253 characters
// /// - no more than 63 characters per label
// /// - no trailing dot
// fn is_hostname(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 253
//         && s.split(|&b| b == b'.').all(|label| {
//             !label.is_empty()
//                 && label.len() <= 63
//                 && !label.starts_with(&[b'-'])
//                 && !label.ends_with(&[b'-'])
//                 && s.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-')
//         })
//         && !s.ends_with(&[b'.'])
// }

// TODO(phlip9): support LNURL pay
struct Lnurl<'a> {
    _url: &'a str,
}

impl Lnurl<'_> {
    // LUD-01: base LNURL bech32 encoding
    fn matches_hrp_prefix(s: &str) -> bool {
        const HRP: &[u8; 6] = b"lnurl1";
        const HRP_LEN: usize = HRP.len();
        match s.as_bytes().split_first_chunk::<HRP_LEN>() {
            Some((s_hrp, _)) => s_hrp.eq_ignore_ascii_case(HRP),
            _ => false,
        }

        // TODO(phlip9): look for "http(s):" scheme with smuggled "lightning"
        // query parameter containing bech32 LNURL

        // TODO(phlip9): look for "lightning:" scheme with bech32 LNURL
    }

    // LUD-17: protocol schemes
    fn matches_scheme(s: &str) -> bool {
        s.eq_ignore_ascii_case("lnurl")
            // LUD-17: fine-grained protocol schemes
            || s.eq_ignore_ascii_case("lnurlc")
            || s.eq_ignore_ascii_case("lnurlw")
            || s.eq_ignore_ascii_case("lnurlp")
            || s.eq_ignore_ascii_case("keyauth")
    }
}

trait AddressExt {
    /// Returns `true` if the given string matches any HRP prefix for BTC
    /// addresses.
    fn matches_hrp_prefix(s: &str) -> bool;

    /// Returns `true` if this address type is supported in a BIP21 URI body.
    fn is_supported_in_uri_body(&self) -> bool;

    /// Returns `true` if this address type is supported in a BIP321 URI query
    /// param.
    fn is_supported_in_uri_query_param(&self) -> bool;
}

impl AddressExt for bitcoin::Address<NetworkUnchecked> {
    fn matches_hrp_prefix(s: &str) -> bool {
        const HRPS: [&[u8]; 3] = [b"bc1", b"tb1", b"bcrt1"];
        let s = s.as_bytes();
        HRPS.iter().any(|hrp| match s.split_at_checked(hrp.len()) {
            Some((prefix, _)) => hrp.eq_ignore_ascii_case(prefix),
            _ => false,
        })
    }

    fn is_supported_in_uri_body(&self) -> bool {
        use bitcoin::AddressType::*;
        let address_type = match self.assume_checked_ref().address_type() {
            Some(x) => x,
            // Non-standard
            None => return true,
        };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => true,
            // Pay to script hash.
            P2sh => true,
            // Pay to witness pubkey hash.
            P2wpkh => true,
            // Pay to witness script hash.
            P2wsh => true,
            // Pay to taproot.
            P2tr => false,
            // Unknown standard
            _ => false,
        }
    }

    fn is_supported_in_uri_query_param(&self) -> bool {
        use bitcoin::AddressType::*;
        let address_type = match self.assume_checked_ref().address_type() {
            Some(x) => x,
            // Non-standard
            None => return true,
        };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => false,
            // Pay to script hash.
            P2sh => false,
            // Pay to witness pubkey hash.
            P2wpkh => true,
            // Pay to witness script hash.
            P2wsh => true,
            // Pay to taproot.
            P2tr => true,
            // Unknown standard
            _ => true,
        }
    }
}

#[cfg(test)]
mod test {
    use common::{
        ln::network::LxNetwork,
        rng::FastRng,
        test_utils::{arbitrary, arbitrary::any_mainnet_addr_unchecked},
        time::TimestampMs,
    };
    use proptest::{
        arbitrary::any, prop_assert_eq, proptest, sample::Index,
        strategy::Strategy,
    };

    use super::*;

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

    #[test]
    fn test_payment_uri_roundtrip() {
        proptest!(|(uri: PaymentUri)| {
            let any_usable = uri.any_usable();
            let actual = PaymentUri::parse(&uri.to_string());
            prop_assert_eq!(Ok(&uri), actual.as_ref());

            let any_usable_via_flatten = !uri.flatten().is_empty();
            prop_assert_eq!(any_usable, any_usable_via_flatten);
        });
    }

    #[test]
    fn test_payment_uri_parse_doesnt_panic() {
        proptest!(|(s: String)| {
            let _ = PaymentUri::parse(&s);
        });
    }

    // cargo test -p payment-uri -- payment_uri_sample --ignored --nocapture
    #[ignore]
    #[test]
    fn payment_uri_sample() {
        let mut rng = FastRng::from_u64(891010909651);
        let strategy = any::<PaymentUri>();
        let value_iter = arbitrary::gen_value_iter(&mut rng, strategy);
        for (idx, value) in value_iter.take(50).enumerate() {
            println!("{idx:>3}: \"{value}\"");
        }
    }

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

    // inserting a `req-` URI param should make us to skip the onchain method
    #[test]
    fn test_bip321_uri_prop_req_param() {
        proptest!(|(uri: Bip321Uri, key: String, value: String, param_idx: Index)| {

            let mut uri_raw = uri.to_uri();
            let param_idx = param_idx.index(uri_raw.params.len() + 1);
            let key = format!("req-{key}");
            let param = UriParam { key: key.into(), value: value.into() };
            uri_raw.params.insert(param_idx, param);

            let actual1 = Bip321Uri::parse(&uri_raw.to_string()).unwrap();
            let actual2 = Bip321Uri::parse_uri(uri_raw).unwrap();
            prop_assert_eq!(&actual1, &actual2);
            prop_assert_eq!(
                Vec::<bitcoin::Address<NetworkUnchecked>>::new(),
                actual1.onchain
            );
            prop_assert_eq!(uri.invoice, actual1.invoice);
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

    #[test]
    fn test_lightning_uri_manual() {
        // small invoice
        let uri_str = "lightning:lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let invoice = &lightning_uri.invoice.unwrap();
        assert_eq!(lightning_uri.offer, None);
        assert_eq!(invoice.amount(), None);
        assert_eq!(invoice.description_str(), None);
        assert_eq!(invoice.network(), LxNetwork::Mainnet.to_bitcoin());
        assert_eq!(
            invoice.created_at().unwrap(),
            TimestampMs::try_from(9412556961000_i64).unwrap(),
        );
        assert_eq!(
            invoice.payee_node_pk().to_string(),
            "031fd9809565f84bdd53c6708a19a8a4952857fb8d68ee1e5af5e8a666d07ef3a7",
        );
        assert_eq!(invoice.0.route_hints().len(), 0);

        // long invoice
        let uri_str = "lightning:lnbc1sd2trl2dk89neerzdaa7amlu9knxnn4malh5jqpmamhle2tr5dym3gpth0h777lwalt5j099a3nr3gpthjk7mm5tp2vg7qnuy7n2gh70l3k27e3s49fmhml0gmc24l9r4xhrcfl9d5vqqqkf32yncmafyca7lm64hjkj76mu5khxk0fp5khnctfpay7x0g88ez8umjv6734rp2tudu3wajgfl0hwll92u3slcfl9d5y3xlr8arhfyjjcmf7z5f82gt7x8trvja003gpt30xuqz5hxghfg0r2us4mcelrulc2alp8u4k32r5gqm7xgmmtgqxqawlwal7wle8ysm7zcg30tn0asv9fg2p9wr6zmq9snv8wpy0dlr4ud4jzeksz0n5x76yaml72h6fdxz5h5fdye07z0etd8shxufjq83nz7ez003jzem2zfza7am7hp2ygnhluyljk60pfa9nsdj9s542ydn5ap0c2208fdtkevsqufzc2jhyzhsn75t0udk56tn6r03jjcgms5m9meg9fycdrjfttqm9cawlwalyle6a25xfplwlwalyheel2vmgumar283s2427cp67wwtpf3afqhq6sjucw78ktsd85xu8rgqyfhqqelzszh9jq8s5x5fuxe07xc6nzg2u2q2uze9yt3gptkghtcfl9d5wul4q23ly32hkrg2dyylr09wnte20wvr72vgh2gpp5q743y4905ggtpapzar8zv3xrkwgfpf7ncq2250kyjgdtxwl7gukqsp53ep2hsj7v0wqy4955x4gfd76qtmehqahledhhey3pjr5ujk4252s9q2sqqqqqysgqcqyp9pknp4qtyrfeasgkr47x72mr6lk0j4665lt4sagdtqark9f3scnymy8cj6zfp4qlwkynsvgrlmy6nkpgv406jccg8fxullw3h9tcqxv5umtx58s20yqr9yq0pf60wnpe294v93fxngec4djvzkqmwu8vpkwecjd7x3xtur8qc4t7dla5tjnkj5f9jnyc5zspavyj6zkqp6n5hpzx8qu69meek3my06q7lznjlhtz6m73ag6lhm3x5t5x0xu2atmnpqnpsefcqedcc8lfz2l02rrsrqmrssz3yqhvejp8r9qpnhea58q8mk890e062tzf9dc5p2sjp56u44zynzdevqfwxxu07kw93g4ex7p2ujd246syj5nlpv5zgv5rfepxygr3rugx2wjm6tgjzm6tu0pv842ajm8uejknh0xlytt95luge0g53ww4y7dzqzcty6fl02cpl4le2ydqdertw2c5g9k664u0revt9h6du0fv6wrszt0qq94kjsw";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let invoice = &lightning_uri.invoice.unwrap();
        assert_eq!(lightning_uri.offer, None);
        assert_eq!(invoice.amount(), None);
        assert_eq!(invoice.description_str().map(|s| s.len()), Some(444));
        assert_eq!(invoice.network(), LxNetwork::Mainnet.to_bitcoin());
        assert_eq!(
            invoice.created_at().unwrap(),
            TimestampMs::try_from(17626927082000_i64).unwrap(),
        );
        assert_eq!(
            invoice.payee_node_pk().to_string(),
            "02c834e7b045875f1bcad8f5fb3e55d6a9f5d61d43560e8ec54c618993643e25a1",
        );
        let route_hints = invoice.0.route_hints();
        assert_eq!(route_hints.len(), 1);
        let route_hint = &route_hints[0];
        assert_eq!(route_hint.0.len(), 2);

        // short offer
        let uri_str = "lightning:lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let offer = &lightning_uri.offer.unwrap();
        assert_eq!(lightning_uri.invoice, None);
        assert!(offer.supports_network(LxNetwork::Mainnet));
        assert_eq!(offer.description(), None);
        assert_eq!(offer.amount(), None);
        assert_eq!(offer.fiat_amount(), None);
        assert_eq!(
            offer.payee_node_pk().unwrap().to_string(),
            "024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8",
        );

        // long offer
        let uri_str = "lightning:lno1pt7srytwezar7l6w2ulj4uv932cu3w327zhmmys2fcqzvt6t9rct4d48tfy098avnujrcfx34rcm3y4pyhec8yyw9lcel8ydyhpt2g0352nf8m5vh8efp09nvf4y7agt726ft26u9megd0u37x6eh9n572464ynm7j9f9xln37uteayx37psmuyn3knj55f69a2rluyljk60r9dd3yer628n3kq2es49724g928zszhrc4mqc2zl8rv53ps3kleh6x50rdaxj3wzjf8nszr2z7734q9nv0wz4vq0p8u4knp2tuvchw50r85ls4zqqz0nkjng3u9usxuk3malhheepvu2xs40fz45hmeemy9t03nl9gyt3ynspsuau2q2u0gd7jqthytq9c94euv6h2njeuyljk6rluyljk683s497zmgtfcqu2cgduyljk6q6tl53zw6au484j5jlu90kw4dr2p9c2jnlc5q4mp222nq7wyfnt2z9qjwl0aapdw0p2vrnnceffagu2q2ac5q4mee4tvf84z8kf6x7xu6rwl34x5c5apw6x5qkempvnhmh0l3hz024s469gn0rw485cn0fq4usaw0p0uzjfvz2flnkk3m7lln5z8m22hs4x4tzukrkthmh0cqpcygqjlqvl3l000tzr7szgsrcc8my32l79fckr7nj63n4f4alqznxem3w0l2er4nd4q9mp7fkuysxv5mxwf0gkjawaz4kxdgdp0dt8uncu9xtmuwl6mrjrnh5jg0tgpdqvpmnqytlhs98r8xynazuqqf3w0f4ps7mchujfkn38nps73dcvrxxuqqxdu0w6z9uplpajgz6ujphevcrccrzav2euc7h9ydctaeg9j8es0e3an5c9js5vmqp6ukr6326ae5e0avhxjs82pepypgz5edrrpjgfhxhccmtq0nuljxdcjr5lxeyg9vqkgtr3e5qqejl7s98yzewt6avqfpscw7chgyty43cmp0vml2qm7uk6gg96tglm2ucqfwjkjsd5rd6smc45gtr3n87p9qxq5era4fcqty72z984af4cgw9pzr8f59908hntawk89k0695ycywtsqpqnfufqzscy7eg3rpgvvuwa0vgfgjl5q6u2sm72hch2eu7xu6t08j5j7cpuvyszuk9uu7hxflfpdds0ef384p7zw6dr367xxmdg8352hev2gdc2jlpwywhgav3wsmu2q2uhnx0qdhzghjkw7e2h8n52f2zwnm72qep9e0tysjutnl7wxmhw0nh6pcurwghg4rfuu5379zdu5ajja4xzfuf0cfl9d5p8p2tuyljk6r3s4yp5aqkwkghtceeqy6u2q2ugf00vh5sr00h77j25x34reehxdjc2jhlj969tcfl9d5u2q2as59a7amluujh7qxnuv24xprmuyljk69crw34rptwth3j6jftuyn3wxlpv5ak9au9f0jn6t2au5m3zgkpu49k7nphu4tnkc09r5u5t32dwpy9tnc9pkghfv4kxextte2nzvzvle6p0yttuh9duy5s2vl9tdkknema09wp8p22wkzhynv9ffucwdn7z0jh7ymhs49x4w08294s0ncmvfx7zsg9xcm9t3gptsmytege9dgytce3w4g9g3v9f0r4j73khzu7wrta0g2a7lmmu4xjk9u9tzs2u70rtv3hqmxsleedagpkfcq9mcfl9d56x50p8u4k33n4udtjk97l0aa7ztc82ef9lef4raj7x5m0fc2pfw08za9kyk0r9dn3yhwpg4r727edv7r5chq62fuptccdv5wrva8k93pqd56rmfv68psmvwzzhse6pj4xg7gu6jrzega4qvzs9f9f4wjvftxj";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let offer = &lightning_uri.offer.unwrap();
        assert_eq!(lightning_uri.invoice, None);
        assert!(offer.supports_network(LxNetwork::Mainnet));
        assert_eq!(offer.description().map(|x| x.len()), Some(401));
        assert_eq!(offer.amount(), None);
        assert_eq!(offer.fiat_amount(), None);
    }

    #[test]
    fn test_lightning_uri_roundtrip() {
        proptest!(|(uri: LightningUri)| {
            let actual = LightningUri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
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

    #[test]
    fn test_parse_err_manual() {
        assert_eq!(
            PaymentUri::parse("philip@lexe.app"),
            Err(ParseError::EmailLookingUnsupported),
        );
        assert_eq!(
            PaymentUri::parse("₿philip@lexe.app"),
            Err(ParseError::Bip353Unsupported),
        );
        assert_eq!(
            PaymentUri::parse("lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
        assert_eq!(
            PaymentUri::parse("lnurl:lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
        assert_eq!(
            PaymentUri::parse("lnurlp:lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
        assert_eq!(
            PaymentUri::parse("lnurlp://lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
    }
}
