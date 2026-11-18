use std::str::FromStr;

use anyhow::{Context, ensure};
use http::{header::HOST, uri::Authority};
use lexe_api::error::{SdkApiError, SdkErrorKind};
use lexe_sha256::sha256;
use reqwest::Url;

/// Which hosts may reach the sidecar and which require bearer auth.
pub(crate) struct HostPolicy {
    default_hosts: AllowedHosts,
    extra_hosts: Option<AllowedHosts>,
}

impl HostPolicy {
    pub(crate) fn new(
        extra_hosts: Option<AllowedHosts>,
        sidecar_url: &str,
    ) -> Self {
        Self {
            default_hosts: AllowedHosts::empty()
                .with_loopback()
                .with_url(sidecar_url),
            extra_hosts,
        }
    }

    /// Rejects invalid or explicitly disallowed hosts.
    ///
    /// Returns `false` for unlisted hosts that may still reach wallet routes
    /// with bearer auth.
    pub(crate) fn allows_implicit_auth(
        &self,
        parts: &http::request::Parts,
    ) -> Result<bool, SdkApiError> {
        // Browsers always send an authority. Allow HTTP/1.0 clients without
        // one (likely not a browser).
        let Some(authority) = request_authority(parts)? else {
            return Ok(true);
        };
        let host = authority.host();
        if self.default_hosts.allows(host) {
            return Ok(true);
        }

        match &self.extra_hosts {
            Some(hosts) if hosts.allows(host) => Ok(true),
            Some(_) => Err(rejection(format!(
                "Host '{host}' is not allowed. To allow it, add it to \
                 --allowed-hosts / $LEXE_SIDECAR_ALLOWED_HOSTS"
            ))),
            None => Ok(false),
        }
    }
}

/// Hostnames allowed in a request's `Host` header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllowedHosts {
    /// Allow any `Host`
    Any,
    /// Lowercase hostnames, without ports.
    Only(Vec<String>),
}

impl AllowedHosts {
    pub(crate) fn empty() -> Self {
        Self::Only(Vec::new())
    }

    pub(crate) fn with_loopback(mut self) -> Self {
        if let Self::Only(hosts) = &mut self {
            hosts.extend(["localhost", "127.0.0.1", "[::1]"].map(String::from));
        }
        self
    }

    pub(crate) fn with_url(mut self, url: &str) -> Self {
        if let Self::Only(hosts) = &mut self
            && let Ok(url) = Url::parse(url)
            && let Some(host) = url.host_str()
        {
            hosts.push(host.to_ascii_lowercase());
        }
        self
    }

    fn allows(&self, host: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Only(hosts) =>
                hosts.iter().any(|h| h.eq_ignore_ascii_case(host)),
        }
    }
}

/// Parses a comma-separated list of hostnames, or `*` to allow any host.
impl FromStr for AllowedHosts {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut hosts = Vec::new();
        for entry in s.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            if entry == "*" {
                return Ok(Self::Any);
            }
            let authority = Authority::from_str(entry)
                .with_context(|| format!("Invalid hostname: '{entry}'"))?;
            ensure!(
                authority.port().is_none(),
                "Allowed host '{entry}' should not include a port",
            );
            hosts.push(authority.host().to_ascii_lowercase());
        }
        Ok(Self::Only(hosts))
    }
}

/// A secret which clients must present as `Authorization: Bearer <token>` to
/// use the default wallet.
#[derive(Clone)]
pub struct ApiToken(sha256::Hash);

impl ApiToken {
    pub(crate) fn matches(&self, token: &str) -> bool {
        // Comparing hashes avoids leaking the token through timing.
        sha256::digest(token.as_bytes()) == self.0
    }
}

/// Parses a token, ignoring surrounding whitespace, e.g. a trailing newline.
impl FromStr for ApiToken {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let token = s.trim();
        ensure!(!token.is_empty(), "API token is empty");
        Ok(Self(sha256::digest(token.as_bytes())))
    }
}

/// The request's `:authority` (HTTP/2) or `Host` header (HTTP/1).
fn request_authority(
    parts: &http::request::Parts,
) -> Result<Option<Authority>, SdkApiError> {
    if let Some(authority) = parts.uri.authority() {
        return Ok(Some(authority.clone()));
    }
    let Some(value) = parts.headers.get(HOST) else {
        return Ok(None);
    };
    Authority::try_from(value.as_bytes())
        .map(Some)
        .map_err(|_| rejection("Invalid Host header".to_owned()))
}

fn rejection(msg: String) -> SdkApiError {
    SdkApiError {
        kind: SdkErrorKind::Rejection,
        msg,
        ..Default::default()
    }
}

#[cfg(test)]
mod test {
    use http::Request;

    use super::*;

    #[test]
    fn allowed_hosts() {
        let url = "http://Sidecar.internal:5393";
        let hosts = AllowedHosts::empty().with_loopback();
        for host in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]"] {
            assert!(hosts.allows(host), "{host}");
        }
        assert!(!hosts.allows("sidecar.internal"));
        assert!(!hosts.allows("evil.example"));
        assert!(!hosts.allows("[::2]"));

        let hosts = AllowedHosts::empty().with_url(url);
        assert!(hosts.allows("sidecar.internal"));
        assert!(!hosts.allows("localhost"));

        let hosts = AllowedHosts::from_str(" Example.com, [::2] ,")
            .unwrap()
            .with_loopback()
            .with_url(url);
        assert!(hosts.allows("example.com"));
        assert!(hosts.allows("[::2]"));
        assert!(hosts.allows("localhost"));
        assert!(hosts.allows("sidecar.internal"));
        assert!(!hosts.allows("evil.example"));

        let any = AllowedHosts::from_str("example.com,*").unwrap();
        assert_eq!(any, AllowedHosts::Any);
        assert!(any.with_loopback().with_url(url).allows("evil.example"));

        AllowedHosts::from_str("example.com:5393").unwrap_err();
        AllowedHosts::from_str("bad/host").unwrap_err();
    }

    #[test]
    fn request_authority_sources() {
        let req = |uri: &str, host: Option<&str>| {
            let mut builder = Request::builder().uri(uri);
            if let Some(host) = host {
                builder = builder.header(HOST, host);
            }
            builder.body(()).unwrap().into_parts().0
        };
        let host = |parts: &http::request::Parts| {
            request_authority(parts)
                .unwrap()
                .map(|a| a.host().to_owned())
        };

        assert_eq!(host(&req("/v2/health", None)), None);
        assert_eq!(
            host(&req("/v2/health", Some("evil.example:5393"))).as_deref(),
            Some("evil.example"),
        );
        assert_eq!(
            host(&req("/v2/health", Some("[::1]:5393"))).as_deref(),
            Some("[::1]"),
        );
        assert_eq!(
            host(&req("http://evil.example/v2/health", Some("localhost")))
                .as_deref(),
            Some("evil.example"),
        );
        request_authority(&req("/v2/health", Some("bad/host"))).unwrap_err();
    }

    #[test]
    fn api_token() {
        let token = ApiToken::from_str(" s3cret\n").unwrap();
        assert!(token.matches("s3cret"));
        assert!(!token.matches(" s3cret"));
        assert!(!token.matches("s3cre"));
        assert!(ApiToken::from_str(" \n").is_err());
    }
}
