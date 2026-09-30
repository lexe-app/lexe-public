use std::{borrow::Cow, fmt, str::FromStr};

use bitcoin::address::NetworkUnchecked;
use lexe_api_core::types::{invoice::Invoice, offer::Offer};
use lexe_common::ln::network::Network;
#[cfg(test)]
use lexe_common::test_utils::arbitrary;
use lexe_connect::request::CredentialRequest;
use lexe_uri::Uri;
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{
    AuthMethod, Error, PaymentMethod, PaymentUriMethods, Resolvable,
    bip321_uri::Bip321Uri,
    email_like::EmailLikeAddress,
    helpers::{self, AddressExt},
    lightning_uri::LightningUri,
    lnurl::Lnurl,
};

/// Refuse to parse any input longer than this many KiB.
const MAX_INPUT_LEN_KIB: usize = 8;

/// A decoded "Payment URI", usually from a scanned QR code, manually pasted
/// code, or handling a URI open (like tapping a `bitcoin:bc1qfjeyfl...` URI in
/// your mobile browser or in another app).
///
/// Many variants give multiple ways to pay, with e.g. BOLT11 invoices including
/// an onchain fallback, or BIP321 URIs including an optional BOLT11 invoice.
#[derive(Debug)]
#[cfg_attr(test, derive(Arbitrary, Eq, PartialEq))]
pub enum PaymentUri {
    /// An BIP321 URI, containing an onchain payment description, plus optional
    /// BOLT11 invoice and/or BOLT12 offer.
    ///
    /// ex: "bitcoin:bc1qfj..."
    ///     "bitcoin:?lno=lno1pqps7..."
    Bip321Uri(Box<Bip321Uri>),

    /// A Lightning URI, containing a BOLT11 invoice or BOLT12 offer.
    ///
    /// ex: "lightning:lnbc1pvjlue..." or
    ///     "lightning:lno1pqps7..."
    LightningUri(LightningUri),

    /// A standalone BOLT11 Lightning invoice.
    ///
    /// ex: "lnbc1pvjlue..."
    Invoice(Invoice),

    /// A standalone BOLT12 Lightning offer.
    ///
    /// ex: "lno1pqps7sj..."
    // TODO(phlip9): BOLT12 refund
    Offer(Offer),

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

    /// An email-like payment address (BIP353 or Lightning Address).
    ///
    /// ex: "satoshi@lexe.app" or "₿satoshi@lexe.app"
    EmailLikeAddress(EmailLikeAddress<'static>),

    /// An LNURL.
    ///
    /// ex: "lnurl1dp68g..." (LUD-01) or "lnurlp://domain.com/path" (LUD-17)
    Lnurl(Lnurl<'static>),

    /// A LexeConnect credential request.
    ///
    /// ex: `https://lexe.app/connect?v=1&redirect_uri=...`
    LexeConnect(Box<CredentialRequest>),
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
    pub fn parse(s: &str) -> Result<Self, Error> {
        // Refuse to parse anything longer than `MAX_LEN_KIB` KiB
        if s.len() > (MAX_INPUT_LEN_KIB << 10) {
            return Err(Error::InvalidPaymentUri(Cow::from(
                "Payment code is too long to parse (>8 KiB)",
            )));
        }

        let s = s.trim();

        // Try parsing a URI-looking thing
        //
        // ex: "bitcoin:bc1qfj..." or
        //     "lightning:lnbc1pvjlue..." or
        //     "https://service.com?lightning=lnurl1dp68g..." or ...
        if let Ok(uri) = Uri::parse(s) {
            // "bitcoin:" with BIP 321 URI: "bitcoin:bc1qfj..."
            if Bip321Uri::matches_uri_scheme(uri.scheme) {
                return Ok(Self::Bip321Uri(Box::new(Bip321Uri::parse_uri(
                    uri,
                ))));
            }

            // ex: "lightning:lnbc1pvjlue..." (BOLT11) or
            //     "lightning:lno1pqps7..." (BOLT12) or
            //     "lightning:lnurl1dp68g..." (bech32 LNURL) or
            //     "lightning:satoshi@lexe.app" (Lightning Address) or
            //     "lightning:₿max@lexe.app" (BIP353)
            if LightningUri::matches_uri_scheme(uri.scheme) {
                return LightningUri::parse_uri(uri).map(Self::LightningUri);
            }

            // Non-bech32 LNURL URI (LUD-17): "lnurlp://domain.com/path"
            if Lnurl::matches_lud17_uri_scheme(uri.scheme)?.is_some() {
                return Lnurl::parse_lud17_uri(uri)
                    .map(Lnurl::into_owned)
                    .map(Self::Lnurl);
            }

            // LexeConnect: "https://lexe.app/connect?v=1&..."
            if s.starts_with(CredentialRequest::LEXE_BASE_URL) {
                return CredentialRequest::parse(s)
                    .map(|request| Self::LexeConnect(Box::new(request)))
                    .map_err(Error::InvalidLexeConnect);
            }

            // LNURL as HTTP query param:
            // "https://service.com?lightning=lnurl1dp68g..."
            if let Some(bech32) = Lnurl::matches_http_with_bech32_param(&uri) {
                return Ok(Self::Lnurl(Lnurl::parse_bech32(&bech32)?));
            }

            return Err(Error::InvalidPaymentUri(Cow::from(
                "Unrecognized URI scheme",
            )));
        }

        // ex: "satoshi+tag@lexe.app" or "₿satoshi@lexe.app" or
        // "%E2%82%BFphilip@lexe.app"
        if let Some((username, domain)) = EmailLikeAddress::matches(s) {
            return EmailLikeAddress::parse_from_parts(username, domain)
                .map(EmailLikeAddress::into_owned)
                .map(Self::EmailLikeAddress);
        }

        // ex: "lnbc1pvjlue..."
        if Invoice::matches_hrp_prefix(s) {
            return Invoice::from_str(s)
                .map(Self::Invoice)
                .map_err(Error::InvalidInvoice);
        }

        // ex: "lno1pqps7sj..."
        if Offer::matches_hrp_prefix(s) {
            return Offer::from_str(s)
                .map(Self::Offer)
                .map_err(Error::InvalidOffer);
        }

        // ex: "lnurl1dp68g..."
        if Lnurl::matches_bech32_hrp_prefix(s) {
            return Ok(Self::Lnurl(Lnurl::parse_bech32(s)?));
        }

        // ex: "bc1qfjeyfl..."
        if bitcoin::Address::matches_hrp_prefix(s) {
            return bitcoin::Address::from_str(s)
                .map(Self::Address)
                .map_err(Error::InvalidBtcAddress);
        }
        // The block above only handles modern bech32 segwit+taproot addresses.
        // We don't have a good way to know ahead of time if this is a legacy
        // bitcoin address or not, so we just have to try but throw away the
        // error.
        if let Ok(address) = bitcoin::Address::from_str(s) {
            return Ok(Self::Address(address));
        }

        Err(Error::InvalidPaymentUri(Cow::from(
            "Unrecognized payment code",
        )))
    }

    /// "Flatten" the [`PaymentUri`] into its directly-known
    /// [`PaymentUriMethods`] and any [`Resolvable`]s requiring further
    /// resolution.
    ///
    /// Filters out onchain addresses that aren't valid for `network`.
    pub fn flatten(
        self,
        network: Network,
    ) -> (PaymentUriMethods, Vec<Resolvable>) {
        let mut methods = PaymentUriMethods::default();
        let mut resolvables = Vec::new();
        match self {
            Self::Bip321Uri(bip321) =>
                (methods.payment_methods, resolvables) =
                    (*bip321).flatten(network),
            Self::LightningUri(lnuri) => {
                let (payment_methods, resolvable) = lnuri.flatten();
                methods.payment_methods = payment_methods;
                resolvables.extend(resolvable);
            }
            Self::Invoice(invoice) =>
                methods.payment_methods = helpers::flatten_invoice(invoice),
            Self::Offer(offer) =>
                methods.payment_methods.push(PaymentMethod::Offer {
                    offer,
                    bip321_amount: None,
                    human_bitcoin_address: None,
                }),
            Self::Address(address) =>
                if let Ok(addr) = address.require_network(network.to_bitcoin())
                {
                    methods.payment_methods.push(PaymentMethod::Onchain {
                        address: addr,
                        amount: None,
                        label: None,
                        message: None,
                    });
                },
            Self::EmailLikeAddress(addr) =>
                resolvables.push(Resolvable::EmailLike(addr)),
            Self::Lnurl(lnurl) => resolvables.push(Resolvable::Lnurl(lnurl)),
            Self::LexeConnect(request) =>
                methods.auth_methods.push(AuthMethod::LexeConnect(request)),
        }
        (methods, resolvables)
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
            Self::EmailLikeAddress(email_like) => Display::fmt(email_like, f),
            Self::Lnurl(lnurl) => Display::fmt(lnurl, f),
            Self::LexeConnect(request) => Display::fmt(request, f),
        }
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::arbitrary;
    use lexe_crypto::rng::FastRng;
    use proptest::{arbitrary::any, prop_assert_eq, prop_assume, proptest};

    use super::*;
    use crate::PaymentUriMethod;

    #[test]
    fn test_payment_uri_roundtrip() {
        proptest!(|(uri1: PaymentUri)| {
            // Skip Lnurl with Https/HttpOnion schemes
            // - plain https:// URLs aren't recognized as LNURLs during parsing,
            // - .onion LNURLs are not supported
            if let PaymentUri::Lnurl(ref lnurl) = uri1 {
                prop_assume!(lnurl.scheme != crate::lnurl::LnurlScheme::Https);
                prop_assume!(lnurl.scheme != crate::lnurl::LnurlScheme::HttpOnion);
            }

            let uri2 = PaymentUri::parse(&uri1.to_string());
            prop_assert_eq!(Ok(&uri1), uri2.as_ref());
        });
    }

    #[test]
    fn test_lexe_connect() {
        let s = "https://lexe.app/connect?v=1\
            &post_url=https%3A%2F%2Fpaygate.com%2Flexe\
            &one_time_secret=000102030405060708090a0b0c0d0e0f\
            &scopes=receive";
        let uri = PaymentUri::parse(s).unwrap();
        assert!(matches!(uri, PaymentUri::LexeConnect(_)));
        assert_eq!(uri.to_string(), s);

        let (methods, resolvables) = uri.flatten(Network::Mainnet);
        assert!(resolvables.is_empty());
        assert!(
            methods.payment_methods.is_empty()
                && methods.claim_methods.is_empty()
        );
        assert_eq!(methods.auth_methods.len(), 1);
        assert_eq!(methods.auth_methods[0].kind(), "lexe-connect");

        let err =
            PaymentUri::parse("https://lexe.app/connect?v=2").unwrap_err();
        assert!(matches!(err, Error::InvalidLexeConnect(_)));
    }

    #[test]
    fn test_payment_uri_parse_doesnt_panic() {
        proptest!(|(s: String)| {
            let _ = PaymentUri::parse(&s);
        });
    }

    // cargo test -p lexe-payment-uri-core -- payment_uri_sample --ignored
    // --nocapture
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
}
