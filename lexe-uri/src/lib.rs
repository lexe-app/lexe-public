//! Minimal URI parsing and encoding: scheme, authority, body, and
//! percent-encoded query params.

use std::{
    borrow::Cow,
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
};

/// A URI failed to parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError(Cow<'static, str>);

impl std::error::Error for ParseError {}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid URI: {}", self.0)
    }
}

/// A raw, parsed URI. The params (both key and value) are percent-encoded. See
/// [URI syntax - RFC 3986](https://datatracker.ietf.org/doc/html/rfc3986).
///
/// ex: `http://example.com/path?foo=bar%20baz`
/// -> Uri {
///     scheme: "http",
///     authority: true,
///     body: "example.com/path",
///     params: [("foo", "bar baz")],
/// }
#[derive(Debug, Eq, PartialEq)]
pub struct Uri<'a> {
    /// e.g. "https", "bitcoin", "lightning"
    pub scheme: &'a str,
    /// [`true`] if this URI had "//" after the `:` scheme separator.
    pub authority: bool,
    /// "example.com/path"
    pub body: Cow<'a, str>,
    pub params: Vec<UriParam<'a>>,
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

    // syntax: "{scheme}:[//]{body}?{key1}={value1}&{key2}={value2}&..."
    pub fn parse(s: &'a str) -> Result<Self, ParseError> {
        /// Maximum length of a URI in bytes.
        const MAX_URI_LEN: usize = 8192;

        // Check URI length limit
        let uri_len = s.len();
        if uri_len > MAX_URI_LEN {
            return Err(ParseError(Cow::from("URI too long (>8192 bytes)")));
        }

        // parse scheme
        // ex: "bitcoin:bc1qfj..." -> `scheme = "bitcoin"`
        let (scheme, rest) = s
            .split_once(':')
            .ok_or_else(|| ParseError(Cow::from("Missing ':' separator")))?;

        // heuristic: limit scheme to 12 characters. If an input exceeds this,
        // then it's probably not a URI.
        if scheme.len() > 12 {
            return Err(ParseError(Cow::from(
                "URI scheme too long (>12 chars)",
            )));
        }

        // ex: "bitcoin:bc1qfj...?message=hello" -> `body = "bc1qfj..."`
        // ex: "http://example.com?foo=bar" -> `body = "example.com"`
        // ex: "http://example.com/foo/bar" -> `body = "example.com/foo/bar"`
        let (body, rest) = rest.split_once('?').unwrap_or((rest, ""));

        // Check if the URI has an authority (starts with "//")
        let (authority, body) = if let Some(stripped) = body.strip_prefix("//")
        {
            (true, stripped)
        } else {
            (false, body)
        };

        // ex: "bitcoin:bc1qfj...?message=hello%20world&amount=0.1"
        //     -> `params = [("message", "hello world"), ("amount", "0.1")]`
        let params = rest
            .split('&')
            .filter_map(UriParam::parse)
            .collect::<Vec<_>>();

        Ok(Self {
            scheme,
            body: Cow::Borrowed(body),
            authority,
            params,
        })
    }

    /// Whether this URI starts with "https://" (case-insensitive).
    pub fn is_https(&self) -> bool {
        self.scheme.eq_ignore_ascii_case("https") && self.authority
    }

    /// Whether this URI starts with "http://" (case-insensitive).
    pub fn is_http(&self) -> bool {
        self.scheme.eq_ignore_ascii_case("http") && self.authority
    }

    /// Append a query param; it is percent-encoded on display.
    pub fn push_param(
        &mut self,
        key: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
    ) {
        self.params.push(UriParam {
            key: key.into(),
            value: value.into(),
        });
    }

    /// The lowercased domain without any trailing dot, e.g. `example.com` for
    /// `https://a@Example.Com.:1/`, or the IP address, e.g. `[::1]` for
    /// `https://[0:0::1]:443/`.
    /// `None` if the URI has:
    /// - no authority, e.g. `mailto:x`
    /// - an empty host
    /// - a non-LDH character, e.g. `аpple.com` with a Cyrillic `а` (vs. its
    ///   punycode `xn--pple-43d.com`)
    /// - a numeric host that isn't a dotted-decimal IPv4 address, e.g.
    ///   `2130706433`
    /// - an IPv4-mapped IPv6 address, e.g. `[::ffff:127.0.0.1]`
    /// - an `xn--` label that decodes to ASCII, e.g. `xn--example-`
    ///
    /// This way, every URI pointing to the domain name `example.com` returns
    /// `"example.com"` or `None`, never another spelling.
    pub fn host(&self) -> Option<Cow<'_, str>> {
        if !self.authority {
            return None;
        }

        // `a@Example.Com:1/` -> `a@Example.Com:1` -> `Example.Com:1`.
        // Browsers and the `url` crate treat `\` like `/` in `https://` urls,
        // so it ends the authority too, else `https://evil.com\@victim.com/`
        // would report `victim.com` while requests go to `evil.com`.
        let authority = self.body.split(['/', '\\', '#']).next()?;
        let host_port =
            authority.rsplit_once('@').map_or(authority, |(_, h)| h);

        // IPv6 literal, e.g. `[0:0::1]:443` -> `[::1]`
        if let Some(v6) = host_port.strip_prefix('[') {
            let (v6, port) = v6.split_once(']')?;
            if !port.is_empty() && !port.starts_with(':') {
                return None;
            }
            let ip = v6.parse::<Ipv6Addr>().ok()?;
            // `Ipv6Addr` and `url` spell IPv4-mapped addresses differently,
            // e.g. `::ffff:1.2.3.4` vs `::ffff:102:304`.
            if ip.to_ipv4_mapped().is_some() {
                return None;
            }
            return Some(Cow::Owned(format!("[{ip}]")));
        }

        // `Example.Com:1` -> `Example.Com`
        let host = host_port.split(':').next()?;

        // IPv4 literal, e.g. `127.0.0.1`. `Ipv4Addr` parses only four decimal
        // octets; other forms, e.g. `0x7f.0.0.1`, are rejected below.
        if host.parse::<Ipv4Addr>().is_ok() {
            return Some(Cow::Borrowed(host));
        }

        // `Example.Com.` -> `Example.Com`
        let host = host.strip_suffix('.').unwrap_or(host);

        // Reject non-LDH characters.
        // TODO(nicole): Encode Unicode domains (IDNs) with punycode.
        let is_ldh = host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
        if !is_ldh {
            return None;
        }

        let host = match host.bytes().any(|b| b.is_ascii_uppercase()) {
            true => Cow::Owned(host.to_ascii_lowercase()),
            false => Cow::Borrowed(host),
        };

        // Reject A-labels that decode to ASCII, e.g. `xn--example-`, which
        // `url` decodes to `example`. A real A-label ends with an encoded
        // non-ASCII character, never the `-` delimiter.
        let is_ascii_a_label =
            |label: &str| label.starts_with("xn--") && label.ends_with('-');
        if host.split('.').any(is_ascii_a_label) {
            return None;
        }

        // Reject other numeric hosts, e.g. `0x7f.0.0.1`, whose last label
        // can't be a TLD (TLDs start with a letter).
        let last_label =
            host.rsplit_once('.').map_or(&*host, |(_, label)| label);
        if !last_label.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return None;
        }

        Some(host)
    }

    /// Whether this URI's [`host`](Self::host) is a `.onion` domain.
    ///
    /// Does NOT check for "http://" or "https://".
    // This is so we can use this method for e.g. "lnurlp://blargh.onion"
    pub fn ends_with_onion(&self) -> bool {
        self.host().is_some_and(|host| host.ends_with(".onion"))
    }
}

// "{scheme}:[//]{body}?{key1}={value1}&{key2}={value2}&..."
impl fmt::Display for Uri<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = self.scheme;
        let scheme_sep = if self.authority { "://" } else { ":" };
        let body = &self.body;
        write!(f, "{scheme}{scheme_sep}{body}")?;

        let mut param_sep: char = '?';
        for param in &self.params {
            write!(f, "{param_sep}{param}")?;
            param_sep = '&';
        }

        Ok(())
    }
}

/// A single `<key>=<value>` URI parameter.
///
/// + Both `key` and `value` are percent-encoded when displayed.
#[derive(Debug, Eq, PartialEq)]
pub struct UriParam<'a> {
    pub key: Cow<'a, str>,
    pub value: Cow<'a, str>,
}

impl<'a> UriParam<'a> {
    pub fn parse(s: &'a str) -> Option<Self> {
        let (key, value) = s.split_once('=')?;
        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8()
            .ok()?;
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        Some(Self { key, value })
    }

    pub fn key_parsed(&'a self) -> UriParamKey<'a> {
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
pub struct UriParamKey<'a> {
    /// The key name. This is case-insensitive.
    ///
    /// ex:     "amount" -> `name = "amount"`
    /// ex:     "AmOuNt" -> `name = "AmOuNt"`
    /// ex: "req-amount" -> `name = "amount"`
    /// ex: "REQ-AMOUNT" -> `name = "AMOUNT"`
    pub name: &'a str,
    /// Whether this key is a required parameter. Required parameters are
    /// prefixed by "req-" (potentially mixed case).
    pub is_req: bool,
}

impl<'a> UriParamKey<'a> {
    pub fn parse(key: &'a str) -> Self {
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

    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{Arbitrary, any},
        collection::vec,
        sample::select,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for Uri<'static> {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_scheme = select(&[
                "https",
                "http",
                "bitcoin",
                "lightning",
                "lnurlp",
                "mailto",
                "a",
                "x-y.z+1",
            ]);
            let any_string =
                || vec(any::<char>(), 0..16).prop_map(String::from_iter);
            // `?` would start the params, and a leading `//` would be read as
            // an authority marker.
            let any_body = any_string().prop_filter(
                "body must not contain `?` or start with `//`",
                |body| !body.contains('?') && !body.starts_with("//"),
            );
            let any_param =
                (any_string(), any_string()).prop_map(|(key, value)| {
                    UriParam {
                        key: Cow::Owned(key),
                        value: Cow::Owned(value),
                    }
                });

            (any_scheme, any::<bool>(), any_body, vec(any_param, 0..4))
                .prop_map(|(scheme, authority, body, params)| Uri {
                    scheme,
                    authority,
                    body: Cow::Owned(body),
                    params,
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use std::env;

    use proptest::{
        arbitrary::any,
        array::uniform8,
        collection::vec,
        prop_assert_eq, prop_oneof, proptest,
        sample::{Index, select},
        strategy::{Just, Strategy},
        test_runner::{Config, TestCaseError},
    };

    use super::*;

    // roundtrip: Uri -> String -> Uri
    #[test]
    fn uri_prop_roundtrip() {
        proptest!(|(uri: Uri<'static>)| {
            let uri_str = uri.to_string();
            let actual = Uri::parse(&uri_str);
            prop_assert_eq!(Ok(uri), actual, " uri_str: {}", uri_str);
        });
    }

    #[test]
    fn param_round_trip() {
        let uri =
            Uri::parse("https://a/cb?e=x%20y%2Bz%26%3D%25&b=&c=1+2").unwrap();
        let values = uri
            .params
            .iter()
            .map(|p| p.value.as_ref())
            .collect::<Vec<_>>();
        // `+` stays literal; a bare key is dropped.
        assert_eq!(values, ["x y+z&=%", "", "1+2"]);
        assert_eq!(
            uri.to_string(),
            "https://a/cb?e=x%20y%2Bz%26%3D%25&b=&c=1%2B2"
        );
        assert!(Uri::parse("https://a/cb?bare").unwrap().params.is_empty());
    }

    #[test]
    fn host() {
        let host = parse_host;
        assert_eq!(
            host("https://BillSplit.com/cb?x=1").as_deref(),
            Some("billsplit.com")
        );
        assert_eq!(
            host("https://user@evil.com:8443/").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            host("https://evil.com\\@victim.com/").as_deref(),
            Some("evil.com")
        );
        assert_eq!(host("https://site.com./").as_deref(), Some("site.com"));
        assert_eq!(
            host("https://xn--pple-43d.com/").as_deref(),
            Some("xn--pple-43d.com")
        );
        assert_eq!(host("https://123.org/").as_deref(), Some("123.org"));
        assert_eq!(host("https://127.0.0.1/").as_deref(), Some("127.0.0.1"));
        assert_eq!(host("https://[0:0::1]:443/").as_deref(), Some("[::1]"));
        assert_eq!(host("https://"), None);
        assert_eq!(host("mailto:x@y"), None);

        // `аpple.com` looks like `apple.com`, but its `а` is Cyrillic.
        let lookalike = "аpple.com";
        assert_ne!(lookalike, "apple.com");
        for uri in [
            format!("https://{lookalike}/").as_str(),
            "https://%65xample.com/",
            "https://2130706433/",
            "https://010.0.0.1/",
            "https://127.1/",
            "https://[::1/",
            "https://[::1]junk/",
            "https://./",
        ] {
            assert_eq!(host(uri), None, "{uri}");
        }
    }

    #[test]
    fn ends_with_onion() {
        let is_onion = |s: &str| Uri::parse(s).unwrap().ends_with_onion();
        for uri in [
            "http://x.onion/",
            "lnurlp://X.ONION./",
            "http://u@x.onion:80/",
            "http://x.onion#y",
        ] {
            assert!(is_onion(uri), "{uri}");
        }
        for uri in [
            "http://x.onion.com/",
            "http://evil.com#.onion",
            "http://x.onion@evil.com/",
            "http://onion/",
        ] {
            assert!(!is_onion(uri), "{uri}");
        }
    }

    /// Concrete spellings where `host()` and `url`, i.e. `reqwest`, might
    /// plausibly disagree.
    #[test]
    fn host_vs_url_crate() {
        let url_host = |s: &str| {
            let url = url::Url::parse(s).ok()?;
            // `url` keeps a trailing dot; `host()` strips it.
            let host = url.host_str()?;
            Some(host.strip_suffix('.').unwrap_or(host).to_owned())
        };

        // Agree: `(uri, host)`
        for (uri, host) in [
            ("https://Example.COM./", "example.com"),
            ("https://a@b@evil.com/", "evil.com"),
            ("https://a\t@evil.com/", "evil.com"),
            ("https://[::1]@evil.com/", "evil.com"),
            ("https://victim.com#@evil.com", "victim.com"),
            ("https://victim.com?@evil.com", "victim.com"),
            ("https://evil.com\\@victim.com/", "evil.com"),
            ("https://[1:0:0:2:0:0:0:3]/", "[1:0:0:2::3]"),
            ("https://[::1.2.3.4]/", "[::102:304]"),
            ("https://a..b/", "a..b"),
        ] {
            assert_eq!(parse_host(uri).as_deref(), Some(host), "{uri:?}");
            assert_eq!(url_host(uri).as_deref(), Some(host), "{uri:?}");
        }

        // `url` reads another host's spelling, so `host()` rejects:
        // `(uri, url's host)`
        for (uri, theirs) in [
            ("https://xn--example-.com/", "example.com"),
            ("https://a.XN--.com/", "a..com"),
            ("https://[::ffff:1.2.3.4]/", "[::ffff:102:304]"),
            ("https://ex%61mple.com/", "example.com"),
            ("https://ex\tample.com/", "example.com"),
            ("https://ｅxample.com/", "example.com"),
            ("https://0x7f.1/", "127.0.0.1"),
            ("https://010.0.0.1/", "8.0.0.1"),
            ("https://1.2.3.4./", "1.2.3.4"),
            ("https:////evil.com/", "evil.com"),
            ("https:evil.com", "evil.com"),
        ] {
            assert_eq!(parse_host(uri), None, "{uri:?}");
            assert_eq!(url_host(uri).as_deref(), Some(theirs), "{uri:?}");
        }

        // `url` rejects, so no request is sent: `(uri, host)`
        for (uri, host) in [
            ("https://xn--abc.com/", "xn--abc.com"),
            ("https://evil.com:x/", "evil.com"),
        ] {
            assert_eq!(parse_host(uri).as_deref(), Some(host), "{uri:?}");
            assert_eq!(url_host(uri), None, "{uri:?}");
        }
    }

    fn parse_host(uri_str: &str) -> Option<String> {
        Uri::parse(uri_str).unwrap().host().map(Cow::into_owned)
    }

    // Differential fuzzing against `url`: each biases the host differently.

    #[test]
    fn host_matches_url_crate() {
        let any_labels =
            vec(select(HOST_ATOMS), 1..4).prop_map(|labels| labels.join("."));
        fuzz_host_matches_url(100_000, any_labels);
    }

    #[test]
    fn numeric_host_matches_url_crate() {
        fuzz_host_matches_url(25_000, any_numeric_host());
    }

    #[test]
    fn ipv6_host_matches_url_crate() {
        fuzz_host_matches_url(25_000, any_ipv6_literal());
    }

    #[test]
    fn a_label_host_matches_url_crate() {
        fuzz_host_matches_url(25_000, any_a_label_host());
    }

    /// Set `URI_FUZZ_CASES` to override `cases`, e.g. for a long run.
    fn fuzz_host_matches_url(
        cases: u32,
        any_host: impl Strategy<Value = String>,
    ) {
        let cases = match env::var("URI_FUZZ_CASES") {
            Ok(cases) => cases.parse().expect("Invalid URI_FUZZ_CASES"),
            Err(_) => cases,
        };
        // Strategies may filter heavily, e.g. `any_a_label_host`.
        let config = Config {
            max_local_rejects: u32::MAX,
            ..Config::with_cases(cases)
        };
        proptest!(config, |(uri_str in any_uri_str(any_host))| {
            assert_host_matches_url(&uri_str)?;
        });
    }

    /// If `host()` and `url` both parse a host, they must agree, else we'd
    /// report a host other than the one requests go to. Also checks the
    /// `https://` re-serialization, which is how LUD-17 URIs are requested.
    fn assert_host_matches_url(uri_str: &str) -> Result<(), TestCaseError> {
        let Ok(mut uri) = Uri::parse(uri_str) else {
            return Ok(());
        };
        assert_url_host(uri.host(), uri_str)?;

        uri.scheme = "https";
        uri.authority = true;
        assert_url_host(uri.host(), &uri.to_string())
    }

    fn assert_url_host(
        ours: Option<Cow<'_, str>>,
        uri_str: &str,
    ) -> Result<(), TestCaseError> {
        let (Some(ours), Ok(url)) = (ours, url::Url::parse(uri_str)) else {
            return Ok(());
        };
        // `url` parses other schemes' hosts differently, e.g. `lnurlp://`.
        if !matches!(url.scheme(), "http" | "https") {
            return Ok(());
        }
        // `host()` strips a trailing dot; `url` keeps it.
        let theirs = url.host_str().map(|h| h.strip_suffix('.').unwrap_or(h));
        prop_assert_eq!(Some(ours.as_ref()), theirs, "uri: {:?}", uri_str);
        Ok(())
    }

    /// A URI string: a scheme prefix, then `any_host` between junk atoms that
    /// stress authority parsing, e.g. userinfo and ports.
    fn any_uri_str(
        any_host: impl Strategy<Value = String>,
    ) -> impl Strategy<Value = String> {
        let any_atom = || {
            let any_char = any::<char>().prop_map(String::from);
            prop_oneof![
                3 => select(HOST_ATOMS).prop_map(str::to_owned),
                5 => select(SYNTAX_ATOMS).prop_map(str::to_owned),
                1 => select(UNICODE_ATOMS).prop_map(str::to_owned),
                1 => any_char,
            ]
        };
        let any_junk = || {
            prop_oneof![
                2 => Just(String::new()),
                1 => vec(any_atom(), 1..6).prop_map(|atoms| atoms.concat()),
            ]
        };

        (select(PREFIXES), any_junk(), any_host, any_junk()).prop_map(
            |(prefix, pre, host, post)| format!("{prefix}{pre}{host}{post}"),
        )
    }

    /// e.g. `0x7f.1`: numbers in the forms `url` may parse as IPv4.
    fn any_numeric_host() -> impl Strategy<Value = String> {
        let any_part = prop_oneof![
            (0..300_u32).prop_map(|n| n.to_string()),
            any::<u32>().prop_map(|n| n.to_string()),
            (0..300_u32).prop_map(|n| format!("0{n:o}")),
            any::<u32>().prop_map(|n| format!("0x{n:x}")),
            any::<u32>().prop_map(|n| format!("0X{n:X}")),
            select(&["", "0", "00", "0x", "08", "a", "1a", "0xg"])
                .prop_map(str::to_owned),
        ];
        let any_parts = prop_oneof![
            any::<u32>().prop_map(|n| Ipv4Addr::from_bits(n).to_string()),
            vec(any_part, 1..6).prop_map(|parts| parts.join(".")),
        ];
        (any_parts, any::<bool>()).prop_map(|(host, dot)| match dot {
            true => host + ".",
            false => host,
        })
    }

    /// e.g. `[::1]`: IPv6 literals, valid or nearly so.
    fn any_ipv6_literal() -> impl Strategy<Value = String> {
        // Valid, spelled by `Ipv6Addr`, in full, or with an IPv4 suffix
        // Repeats bias toward `::` and IPv4-mapped addresses.
        let any_segment = select(&[0_u16, 0, 0, 1, 0x7f00, 0xffff, 0xffff]);
        let any_valid = (uniform8(any_segment), 0..3_u8).prop_map(
            |(segments, spelling)| {
                let hex = |segments: &[u16]| {
                    segments
                        .iter()
                        .map(|s| format!("{s:X}"))
                        .collect::<Vec<_>>()
                        .join(":")
                };
                let ip = Ipv6Addr::from(segments);
                match spelling {
                    0 => ip.to_string(),
                    1 => hex(&segments),
                    _ => {
                        let v4 = Ipv4Addr::from_bits(ip.to_bits() as u32);
                        format!("{}:{v4}", hex(&segments[..6]))
                    }
                }
            },
        );
        // Random pieces; empty ones join into `::`
        let any_piece = prop_oneof![
            select(&[
                "",
                "0",
                "1",
                "0000",
                "00001",
                "fFfF",
                "7f00",
                "g",
                "1.2.3.4",
                "01.2.3.4",
                "1.2.3",
                "256.0.0.1",
            ])
            .prop_map(str::to_owned),
            any::<u16>().prop_map(|n| format!("{n:x}")),
        ];
        let any_pieces =
            vec(any_piece, 1..10).prop_map(|pieces| pieces.join(":"));

        prop_oneof![any_valid, any_pieces].prop_map(|ip| format!("[{ip}]"))
    }

    /// e.g. `xn--pple-43d.com`: a valid A-label from `url`, maybe mutated.
    fn any_a_label_host() -> impl Strategy<Value = String> {
        let any_u_label =
            vec(select(U_LABEL_CHARS), 1..6).prop_map(String::from_iter);
        let any_mutation = (0..5_u8, any::<Index>(), select(PUNYCODE_CHARS));

        (any_u_label, any_mutation, select(&["", ".com"])).prop_filter_map(
            "No A-label",
            |(u_label, (mutation, index, c), tld)| {
                let url =
                    url::Url::parse(&format!("https://{u_label}/")).ok()?;
                let a_label = url.host_str()?.strip_prefix("xn--")?;
                let mut chars = a_label.chars().collect::<Vec<_>>();
                let i = index.index(chars.len());
                match mutation {
                    0 => (),
                    1 => chars.iter_mut().for_each(char::make_ascii_uppercase),
                    2 => chars[i] = c,
                    3 => chars.insert(index.index(chars.len() + 1), c),
                    _ => drop(chars.remove(i)),
                }
                let a_label = String::from_iter(chars);
                Some(format!("xn--{a_label}{tld}"))
            },
        )
    }

    const PREFIXES: &[&str] = &[
        "https://",
        "http://",
        "HtTpS://",
        "https:",
        " https://",
        "\thttps://",
        "ht\ntps://",
        "lnurlp://",
        "LNURLW:",
    ];
    const HOST_ATOMS: &[&str] = &[
        "a",
        "Z",
        "example",
        "com",
        "onion",
        "localhost",
        "xn--pple-43d",
        "XN--",
        "xn--a-",
        "0",
        "1",
        "08",
        "127",
        "255",
        "256",
        "4294967296",
        "0x7f",
        "0X",
        "ffff",
        "1.2.3.4",
    ];
    const SYNTAX_ATOMS: &[&str] = &[
        ".", "-", "_", ":", "::", "@", "[", "]", "/", "\\", "#", "?", "%",
        "%2e", "%2E", "%40", "%2f", "%5c", "%00", "%25", " ", "\t", "\n", "\r",
        "\0", "\x7f", ":443", ":0x1",
    ];
    /// Unicode that IDNA maps, ignores, or rejects
    const UNICODE_ATOMS: &[&str] = &[
        "а", "ß", "ｅ", "\u{ad}", "\u{200b}", "\u{3002}", "\u{ff0e}",
        "\u{212a}", "\u{fe0f}", "İ",
    ];
    /// ASCII, letters, scripts, marks, joiners, bidi, and mapped characters
    const U_LABEL_CHARS: &[char] = &[
        'a', '0', '-', 'ä', 'é', 'ß', 'ς', 'а', 'я', '中', '😀', '\u{301}',
        '\u{200d}', 'ب', 'א', 'ı', 'ǅ', 'Ⅻ', '\u{fe0f}',
    ];
    const PUNYCODE_CHARS: &[char] = &['a', 'k', 'z', 'A', '0', '9', '-'];
}
