//! Minimal URI parsing and encoding: scheme, authority, body, and
//! percent-encoded query params.
//!
//! Hosts are handled as ASCII only: there is no IDNA / punycode mapping, so
//! `https://bücher.de/` is neither normalized to `xn--bcher-kva.de` nor
//! rejected. Compare hosts only against ASCII values.

use std::{borrow::Cow, fmt};

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

    /// The lowercased host, e.g. `example.com` for `https://a@Example.Com:1/`.
    /// `None` if there is no authority (`mailto:x`) or the host is empty.
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

        // `Example.Com:1` -> `Example.Com`
        let host = match host_port.strip_prefix('[') {
            // IPv6 literal, e.g. `[::1]:443`
            Some(v6) => v6.split(']').next()?,
            None => host_port.split(':').next()?,
        };

        if host.is_empty() {
            return None;
        }
        let lowercased = match host.bytes().any(|b| b.is_ascii_uppercase()) {
            true => Cow::Owned(host.to_ascii_lowercase()),
            false => Cow::Borrowed(host),
        };
        Some(lowercased)
    }

    /// Whether this URI's domain ends with ".onion" (case-insensitive).
    ///
    /// Does NOT check for "http://" or "https://".
    // This is so we can use this method for e.g. "lnurlp://blargh.onion"
    pub fn ends_with_onion(&self) -> bool {
        const ONION_SUFFIX: &str = ".onion";
        const ONION_SUFFIX_LEN: usize = ONION_SUFFIX.len();

        // Extract just the domain part (before any path)
        let domain = self
            .body
            .split_once('/')
            .map(|(domain, _path)| domain)
            .unwrap_or(self.body.as_ref());

        let suffix =
            match domain.as_bytes().split_last_chunk::<ONION_SUFFIX_LEN>() {
                Some((_, s)) => s,
                _ => return false,
            };

        suffix.eq_ignore_ascii_case(ONION_SUFFIX.as_bytes())
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
    use proptest::{prop_assert_eq, proptest};

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
    fn host() {
        let host = |s: &str| Uri::parse(s).unwrap().host().map(Cow::into_owned);
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
        assert_eq!(host("https://[::1]:443/").as_deref(), Some("::1"));
        assert_eq!(host("https://"), None);
        assert_eq!(host("mailto:x@y"), None);
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
}
