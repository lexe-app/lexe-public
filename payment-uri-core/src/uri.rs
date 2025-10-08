use std::{borrow::Cow, fmt};

use crate::Error;

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
#[derive(Debug)]
pub(crate) struct Uri<'a> {
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
    pub fn parse(s: &'a str) -> Result<Self, Error> {
        /// Maximum length of a URI in bytes.
        const MAX_URI_LEN: usize = 4096;

        // Check URI length limit
        let uri_len = s.len();
        if uri_len > MAX_URI_LEN {
            return Err(Error::InvalidUri(Cow::from(
                "URI too long (>4096 bytes)",
            )));
        }

        // parse scheme
        // ex: "bitcoin:bc1qfj..." -> `scheme = "bitcoin"`
        let (scheme, rest) = s.split_once(':').ok_or_else(|| {
            Error::InvalidUri(Cow::from("Missing ':' separator"))
        })?;

        // heuristic: limit scheme to 12 characters. If an input exceeds this,
        // then it's probably not a URI.
        if scheme.len() > 12 {
            return Err(Error::InvalidUri(Cow::from(
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
#[derive(Debug)]
pub(crate) struct UriParam<'a> {
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
pub(crate) struct UriParamKey<'a> {
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
