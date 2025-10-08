//! LNURL parsing and validation.
//!
//! LNURL is a protocol for interacting with Lightning services via HTTP(S)
//! URLs. Services provide functionality for:
//! - **Pay** (LUD-06): Pay to static QR codes with rich metadata
//! - **Withdraw** (LUD-03): Withdraw funds from services via QR code
//! - **Channel** (LUD-02): Request incoming payment channels
//! - **Auth** (LUD-04): Login/authentication with Bitcoin wallet
//!
//! ## Supported formats
//!
//! This module implements parsing of LNURLs from various formats:
//!
//! **LUD-17 URIs**:
//!
//! Example: `lnurlp://example.com/path` is parsed and converted to
//! `https://example.com/path`. The original scheme is preserved in
//! [`Lnurl::scheme`] to help resolvers to determine intent.
//!
//! **Bech32-encoded LNURLs** (LUD-01):
//!
//! Technically deprecated by LUD-17 but still broadly supported.
//!
//! - Direct bech32: `lnurl1dp68gurn8ghj7...`
//! - In a `lightning` URI param: `https://service.com?lightning=lnurl1...`
//! - As the body of a `lightning:` URI: `lightning:lnurl1dp68gurn8ghj7...`
//!
//! Tor `.onion` LNURLs are not currently supported.
//!
//! ## Resolvers
//!
//! Resolvers should make a request to [`Lnurl::http_url`] to retrieve the
//! PayRequest, etc.
//!
//! ## AI context prompt
//!
//! ```md
//! Please read these LUD specs:
//!
//! - `luds/01.md` (Base LNURL encoding and decoding)
//! - `luds/02.md` (LNURL-Channel)
//! - `luds/03.md` (LNURL-Withdraw)
//! - `luds/04.md` (LNURL-Auth)
//! - `luds/05.md` (`linkingKey` derivation)
//! - `luds/06.md` (LNURL-Pay)
//! - `luds/16.md` (Lightning Address)
//! - `luds/17.md` (URI encoding of LNURLs)
//! - `lightning-address/DIY.md`
//! ```

use std::{borrow::Cow, fmt};

use bech32::{Bech32, Hrp};
#[cfg(test)]
use proptest::{
    arbitrary::Arbitrary,
    strategy::{BoxedStrategy, Strategy},
};
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{uri::Uri, Error};

/// A parsed LNURL.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct Lnurl<'a> {
    /// The decoded HTTP URL that the LNURL points to.
    ///
    /// Guaranteed to be `https://` or `http://` with `.onion`,
    /// although `.onion` LNURLs are currently rejected.
    /// The scheme is also normalized to lowercase.
    pub http_url: Cow<'a, str>,
    /// The original LNURL scheme encountered during parsing.
    /// Resolvers may use this to determine intent.
    pub scheme: LnurlScheme,
    /// Optional "tag" query parameter contained in the HTTP URL - see LUD-01.
    pub tag: Option<Cow<'a, str>>,
}

/// The LNURL scheme encountered during parsing.
/// Resolvers may use this to determine intent.
///
/// LUD-17 defines fine-grained protocol schemes for specific flows:
/// `lnurlp://`, `lnurlw://`, `lnurlc://`, and `keyauth://` - see below.
#[derive(Copy, Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq, Arbitrary))]
pub enum LnurlScheme {
    /// `https://`
    Https,
    /// `http://example.onion`
    HttpOnion,
    /// `lnurlp://`  (LUD-06 `payRequest`, LNURL-pay)
    Pay,
    /// `lnurlw://`  (LUD-03 `withdrawRequest`, LNURL-withdraw)
    Withdraw,
    /// `lnurlc://`  (LUD-02 `channelRequest`, LNURL-channel)
    Channel,
    /// `keyauth://` (LUD-04 `login`, LNURL-auth)
    Auth,
}

impl<'a> Lnurl<'a> {
    /// The Human Readable Part for bech32-encoded LNURLs.
    const HRP: Hrp = Hrp::parse_unchecked(Self::HRP_STR);
    const HRP_STR: &'static str = "lnurl";

    pub fn into_owned(self) -> Lnurl<'static> {
        Lnurl {
            http_url: Cow::Owned(self.http_url.into_owned()),
            scheme: self.scheme,
            tag: self.tag.map(|t| Cow::Owned(t.into_owned())),
        }
    }

    /// Parse a LNURL from various formats.
    ///
    /// Accepts:
    /// - Bech32-encoded: `lnurl1dp68...`
    /// - LUD-17 URIs: `lnurlp://domain.com/path`, `lnurlw://...`, etc.
    /// - HTTPS with bech32 param: `https://service.com?lightning=lnurl1...`
    /// - Lightning with bech32 body: `lightning:lnurl1...`
    ///
    /// Returns an error if the input doesn't match any LNURL format.
    pub fn parse(s: &str) -> Result<Lnurl<'static>, Error> {
        let s = s.trim();

        if let Ok(uri) = Uri::parse(s) {
            // lnurlp://, lnurlw://, lnurlc://, keyauth://
            if Lnurl::matches_lud17_uri_scheme(uri.scheme)?.is_some() {
                return Lnurl::parse_lud17_uri(uri).map(Lnurl::into_owned);
            }

            // `https://service.com?lightning=lnurl1...`
            if let Some(bech32) = Lnurl::matches_http_with_bech32_param(&uri) {
                return Lnurl::parse_bech32(&bech32);
            }

            // `lightning:lnurl1...`
            if let Some(bech32) =
                Lnurl::matches_lightning_with_bech32_body(&uri)
            {
                return Lnurl::parse_bech32(&bech32);
            }
        }

        // `lnurl1dp68...`
        if Lnurl::matches_bech32_hrp_prefix(s) {
            return Lnurl::parse_bech32(s);
        }

        Err(Error::InvalidLnurl(Cow::from(
            "Input does not match any supported LNURL format",
        )))
    }

    /// Convert to a bech32-encoded LNURL.
    pub fn to_bech32(&self) -> Result<String, Error> {
        let encoded =
            bech32::encode::<Bech32>(Self::HRP, self.http_url.as_bytes())
                // TODO(max): The Error::Lnurl returns a parsing error
                .map_err(|e| Error::InvalidLnurl(Cow::from(e.to_string())))?;

        Ok(encoded)
    }

    /// Whether a string looks like a bech32 encoded LNURL.
    pub(crate) fn matches_bech32_hrp_prefix(s: &str) -> bool {
        // bech32 hrp is "lnurl", case-insensitive, with '1' separator
        const PREFIX: &str = "lnurl1";
        const PREFIX_LEN: usize = PREFIX.len();

        let s_prefix = match s.as_bytes().split_first_chunk::<PREFIX_LEN>() {
            Some((p, _)) => p,
            _ => return false,
        };

        s_prefix.eq_ignore_ascii_case(PREFIX.as_bytes())
    }

    /// Whether the given URI scheme matches LNURL protocol schemes.
    ///
    /// Returns [`Err`] if we see `lnurl://`, which is not in the spec,
    /// `Ok(Some(_))` for `lnurlp://`, `lnurlw://`, `lnurlc://`, `keyauth://`
    /// (valid LUD-17 schemes), and `Ok(None)` otherwise.
    ///
    /// Does not return [`LnurlScheme::Https`] or [`LnurlScheme::HttpOnion`].
    pub(crate) fn matches_lud17_uri_scheme(
        scheme: &str,
    ) -> Result<Option<LnurlScheme>, Error> {
        // lnurl:// isn't in the spec, but it's strong evidence of intent to
        // interpret as LNURL. Reject here with an error.
        if scheme.eq_ignore_ascii_case("lnurl") {
            return Err(Error::InvalidLnurl(Cow::from(
                "'lnurl://' is not in the spec; \
                 use 'lnurlp://', 'lnurlw://', 'lnurlc://', or 'keyauth://'",
            )));
        }

        if scheme.eq_ignore_ascii_case("lnurlp") {
            Ok(Some(LnurlScheme::Pay))
        } else if scheme.eq_ignore_ascii_case("lnurlw") {
            Ok(Some(LnurlScheme::Withdraw))
        } else if scheme.eq_ignore_ascii_case("lnurlc") {
            Ok(Some(LnurlScheme::Channel))
        } else if scheme.eq_ignore_ascii_case("keyauth") {
            Ok(Some(LnurlScheme::Auth))
        } else {
            Ok(None)
        }
    }

    /// Check if the given URI contains a `lightning=` query param containing a
    /// bech32-encoded LNURL. Returns the bech32 string if found.
    ///
    /// Example: `https://service.com/pay?lightning=lnurl1getrekt...`
    pub(crate) fn matches_http_with_bech32_param<'param>(
        uri: &Uri<'param>,
    ) -> Option<Cow<'param, str>> {
        if !uri.is_http() && !uri.is_https() {
            return None;
        }

        // Check for `lightning=` param with bech32 value with "lnurl" HRP
        uri.params.iter().find_map(|param| {
            if param.key_parsed().is("lightning")
                && Self::matches_bech32_hrp_prefix(&param.value)
            {
                Some(param.value.clone())
            } else {
                None
            }
        })
    }

    /// Check for `lightning:` followed by a bech32-encoded LNURL.
    /// Returns the bech32 string if found.
    ///
    /// Example: `lightning:lnurl1dp68gurn...`
    pub(crate) fn matches_lightning_with_bech32_body<'body>(
        uri: &Uri<'body>,
    ) -> Option<Cow<'body, str>> {
        if !uri.scheme.eq_ignore_ascii_case("lightning") {
            return None;
        }

        if Self::matches_bech32_hrp_prefix(&uri.body) {
            Some(uri.body.clone())
        } else {
            None
        }
    }

    /// Parse a LNURL from a bech32 string.
    ///
    /// Example: `lnurl1dp68...`
    pub(crate) fn parse_bech32(s: &str) -> Result<Lnurl<'static>, Error> {
        let (hrp, bytes) = bech32::decode(s)
            .map_err(|e| format!("Bech32 decode error: {e}"))
            .map_err(|e| Error::InvalidLnurl(Cow::from(e)))?;

        if !hrp.as_str().eq_ignore_ascii_case(Self::HRP_STR) {
            return Err(Error::InvalidLnurl(Cow::from(format!(
                "Invalid LNURL HRP: expected 'lnurl', got '{hrp}'",
            ))));
        }

        let http_url = String::from_utf8(bytes)
            .map_err(|e| format!("Doesn't contain valid UTF-8: {e}"))
            .map_err(|e| Error::InvalidLnurl(Cow::from(e)))?;

        Self::from_http_url(&http_url).map(|lnurl| lnurl.into_owned())
    }

    /// Parse a LUD-17 LNURL URI.
    /// These are converted to HTTPS URLs per LUD-17.
    ///
    /// Example: `lnurlp://domain.com/path?foo=bar`
    pub(crate) fn parse_lud17_uri(
        mut uri: Uri<'a>,
    ) -> Result<Lnurl<'a>, Error> {
        let scheme =
            Self::matches_lud17_uri_scheme(uri.scheme)?.ok_or_else(|| {
                Error::InvalidLnurl(Cow::from("Not a valid LNURL URI"))
            })?;

        if uri.ends_with_onion() {
            return Err(Error::InvalidLnurl(Cow::from(
                "Tor (.onion) addresses are not supported",
            )));
        }

        // Reconstruct the HTTPS URI, validate it, and convert back to HTTP.
        // "https://example.com/path?foo=bar"
        uri.scheme = "https";
        uri.authority = true;
        debug_assert!(uri.is_https());
        Self::validate_http_uri(&uri)?;
        let http_url = Cow::Owned(uri.to_string());

        // Extract the tag query parameter if present (LUD-01)
        let tag = uri.params.into_iter().find_map(|param| {
            if param.key_parsed().is("tag") {
                Some(param.value)
            } else {
                None
            }
        });

        Ok(Self {
            http_url,
            scheme,
            tag,
        })
    }

    /// Construct a [`Lnurl`] from a cleartext HTTP URL.
    ///
    /// Example: `https://service.com/path?foo=bar`
    pub(crate) fn from_http_url<'b>(
        http_url: &'b str,
    ) -> Result<Lnurl<'b>, Error> {
        let mut http_uri = Uri::parse(http_url)
            .map_err(|e| Error::InvalidLnurl(Cow::from(e.to_string())))?;
        let scheme = Self::validate_http_uri(&http_uri)?;

        // Normalize scheme to lowercase if needed
        let http_url = if http_uri.scheme.chars().any(|c| c.is_uppercase()) {
            http_uri.scheme =
                if http_uri.is_https() { "https" } else { "http" };
            Cow::Owned(http_uri.to_string())
        } else {
            Cow::Borrowed(http_url)
        };

        // Extract the tag query parameter if present (LUD-01)
        let tag = http_uri.params.iter().find_map(|param| {
            if param.key_parsed().is("tag") {
                Some(param.value.clone())
            } else {
                None
            }
        });

        Ok(Lnurl {
            http_url,
            scheme,
            tag,
        })
    }

    /// Validates an `http(s)://` LNURL URI.
    fn validate_http_uri(uri: &Uri) -> Result<LnurlScheme, Error> {
        // Technically, LNURL allows "http://" with .onion addresses, but until
        // a user asks us for Tor support, we will simply not support this.
        if uri.is_http() && uri.ends_with_onion() {
            // return Ok(LnurlScheme::HttpOnion);
            return Err(Error::InvalidLnurl(Cow::from(
                ".onion LNURLs are not supported",
            )));
        }

        if !uri.is_https() {
            return Err(Error::InvalidLnurl(Cow::from(
                "LNURL must start with https://",
            )));
        }

        Ok(LnurlScheme::Https)
    }
}

impl<'a> fmt::Display for Lnurl<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = match self.scheme {
            LnurlScheme::Https => "https",
            LnurlScheme::HttpOnion => "http",
            LnurlScheme::Pay => "lnurlp",
            LnurlScheme::Withdraw => "lnurlw",
            LnurlScheme::Channel => "lnurlc",
            LnurlScheme::Auth => "keyauth",
        };

        let rest = self
            .http_url
            .strip_prefix("https://")
            .or_else(|| self.http_url.strip_prefix("http://"))
            .expect("http_url should start with https:// or http://");

        write!(f, "{scheme}://{rest}")
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use proptest::prelude::any;

    use super::*;

    impl<'a> Arbitrary for Lnurl<'a> {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (any::<LnurlScheme>(), any_https_url())
                .prop_map(|(mut scheme, url)| {
                    // If we generate HttpOnion with a non-.onion URL, coerce to
                    // Https to avoid invalid combinations. We only generate
                    // regular https:// URLs, but we want to keep HttpOnion in
                    // the enum for future-proofing.
                    if scheme == LnurlScheme::HttpOnion {
                        scheme = LnurlScheme::Https;
                    }

                    Lnurl {
                        http_url: Cow::Owned(url),
                        scheme,
                        tag: None,
                    }
                })
                .boxed()
        }
    }

    /// Generate valid HTTPS URLs for testing.
    fn any_https_url() -> impl Strategy<Value = String> {
        // Simple URL generation for testing
        (
            // Domain
            "[a-z]{3,10}\\.(com|org|app|io)",
            // Path
            "(/[a-z]{2,8}){0,3}",
            // Query params
            "(\\?[a-z]{2,5}=[a-z0-9]{3,10}(&[a-z]{2,5}=[a-z0-9]{3,10}){0,2})?",
        )
            .prop_map(|(domain, path, query)| {
                format!("https://{}{}{}", domain, path, query)
            })
    }
}

#[cfg(test)]
mod test {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn prop_lnurl_roundtrip() {
        proptest!(|(lnurl1: Lnurl<'static>)| {
            // Https and HttpOnion schemes don't roundtrip through Display+Parse
            // since plain https:// URLs aren't recognized as LNURLs, and .onion
            // LNURLs are rejected. Use prop_bech32_roundtrip to test Https scheme
            // roundtripping via bech32 encoding.
            prop_assume!(lnurl1.scheme != LnurlScheme::Https);
            prop_assume!(lnurl1.scheme != LnurlScheme::HttpOnion);

            let s = lnurl1.to_string();
            let lnurl2 = Lnurl::parse(&s).unwrap();
            prop_assert_eq!(lnurl1, lnurl2);
        });
    }

    #[test]
    fn prop_bech32_roundtrip() {
        proptest!(|(lnurl1: Lnurl<'static>)| {
            // Coerce to Https
            let mut lnurl1 = lnurl1;
            lnurl1.scheme = LnurlScheme::Https;

            let bech32 = lnurl1.to_bech32().unwrap();
            let lnurl2 = Lnurl::parse_bech32(&bech32).unwrap();
            prop_assert_eq!(lnurl1, lnurl2);
        });
    }

    #[test]
    fn test_parse_bech32() {
        // Valid cases
        let valid = [
            "lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns",
            "LNURL1DP68GURN8GHJ7UM9WFMXJCM99E3K7MF0V9CXJ0M385EKVCENXC6R2C35XVUKXEFCV5MKVV34X5EKZD3EV56NYD3HXQURZEPEXEJXXEPNXSCRVWFNV9NXZCN9XQ6XYEFHVGCXXCMYXYMNSERXFQ5FNS",
        ];
        for s in valid {
            let lnurl = Lnurl::parse_bech32(s).unwrap();
            assert!(lnurl.http_url.starts_with("https://"));
        }

        // Invalid cases
        let invalid = ["lnbc1234567890", "lnurl1invalid!@#$", "", "lnurl"];
        for s in invalid {
            assert!(Lnurl::parse_bech32(s).is_err());
        }
    }

    #[test]
    fn test_parse_lud17_uri() {
        let lnurl = Lnurl::parse("lnurlp://example.com/pay").unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Pay);
        assert_eq!(lnurl.http_url, "https://example.com/pay");

        let lnurl =
            Lnurl::parse("lnurlw://example.org/withdraw?amount=100").unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Withdraw);
        assert_eq!(lnurl.http_url, "https://example.org/withdraw?amount=100");

        let lnurl = Lnurl::parse("lnurlc://service.io/channel").unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Channel);
        assert_eq!(lnurl.http_url, "https://service.io/channel");

        let lnurl = Lnurl::parse("keyauth://auth.app/login").unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Auth);
        assert_eq!(lnurl.http_url, "https://auth.app/login");

        // Invalid: lnurl:// is not in spec
        assert!(Lnurl::parse("lnurl://example.com").is_err());
    }

    #[test]
    fn test_parse_https_with_bech32_param() {
        let bech32 = "lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns";
        let uri = format!("https://service.com/pay?lightning={bech32}");

        let lnurl = Lnurl::parse(&uri).unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Https);
        assert!(lnurl.http_url.starts_with("https://"));
    }

    #[test]
    fn test_parse_lightning_with_bech32() {
        let bech32 = "lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns";
        let uri = format!("lightning:{bech32}");

        let lnurl = Lnurl::parse(&uri).unwrap();
        assert_eq!(lnurl.scheme, LnurlScheme::Https);
        assert!(lnurl.http_url.starts_with("https://"));
    }

    #[test]
    fn test_bech32_encode_decode_roundtrip() {
        let test_urls = [
            "https://service.com/api/v1/lnurl",
            "https://example.org/pay?amount=100",
            "https://lightning.app/withdraw",
        ];

        for url in test_urls {
            // Encode to bech32
            let lnurl = Lnurl::from_http_url(url).unwrap();
            let encoded = lnurl.to_bech32().unwrap();
            assert!(encoded.starts_with("lnurl1"));

            // Decode back
            let decoded = Lnurl::parse_bech32(&encoded).unwrap();
            assert_eq!(decoded.http_url, url);
        }
    }

    #[test]
    fn test_validate_http_uri() {
        // Valid URLs
        let uri = Uri::parse("https://example.com").unwrap();
        assert!(Lnurl::validate_http_uri(&uri).is_ok());
        let uri = Uri::parse("https://api.service.org/path").unwrap();
        assert!(Lnurl::validate_http_uri(&uri).is_ok());

        // Invalid URLs
        let uri = Uri::parse("http://example.com").unwrap();
        assert!(Lnurl::validate_http_uri(&uri).is_err()); // Not HTTPS
        let uri = Uri::parse("http://something.onion/path").unwrap();
        assert!(Lnurl::validate_http_uri(&uri).is_err()); // .onion not supported
        let uri = Uri::parse("ftp://example.com").unwrap();
        assert!(Lnurl::validate_http_uri(&uri).is_err()); // Wrong scheme
        assert!(Uri::parse("example.com").is_err()); // No scheme

        // URI too long (>4096 bytes)
        let long_url = format!("https://example.com/{}", "x".repeat(4100));
        assert!(Uri::parse(&long_url).is_err());
    }

    #[test]
    fn test_lnurl_display() {
        // LNURL should display as the decoded URL
        let url = "https://example.com/lnurl";
        let lnurl = Lnurl::from_http_url(url).unwrap();
        let encoded = lnurl.to_bech32().unwrap();
        let lnurl = Lnurl::parse_bech32(&encoded).unwrap();
        assert_eq!(lnurl.to_string(), url);

        // Clear text URL should display as is
        let lnurl = Lnurl::from_http_url(url).unwrap();
        assert_eq!(lnurl.to_string(), url);
    }
}
