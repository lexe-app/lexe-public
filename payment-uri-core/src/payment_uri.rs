use std::{fmt, str::FromStr};

use bitcoin::address::NetworkUnchecked;
#[cfg(test)]
use common::test_utils::arbitrary;
use lexe_api_core::types::{invoice::LxInvoice, offer::LxOffer};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{
    bip321_uri::Bip321Uri,
    email_like::{Bip353Address, EmailLikeAddress},
    helpers::{self, AddressExt},
    lightning_uri::LightningUri,
    lnurl::Lnurl,
    payment_method::{Onchain, PaymentMethod},
    uri::Uri,
    ParseError, MAX_INPUT_LEN_KIB,
};

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
    // EmailLikeAddress(EmailLikeAddress),
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
        if s.len() > (MAX_INPUT_LEN_KIB << 10) {
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
        if let Some((_local, _domain)) = EmailLikeAddress::matches(s) {
            return Err(ParseError::EmailLikeUnsupported);
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
                helpers::flatten_invoice_into(invoice, &mut out);
                out
            }
            Self::Offer(offer) => vec![PaymentMethod::Offer(offer)],
            Self::Address(address) =>
                vec![PaymentMethod::Onchain(Onchain::from(address))],
        }
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

#[cfg(test)]
mod test {
    use common::{rng::FastRng, test_utils::arbitrary};
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

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
    fn test_parse_err_manual() {
        assert_eq!(
            PaymentUri::parse("philip@lexe.app"),
            Err(ParseError::EmailLikeUnsupported),
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
