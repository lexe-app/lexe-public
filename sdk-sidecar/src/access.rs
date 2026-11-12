use std::{str::FromStr, sync::Arc};

use anyhow::{Context, ensure};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use http::{
    HeaderValue,
    header::{AUTHORIZATION, HOST, ORIGIN},
    uri::Authority,
};
use lexe_api::error::{SdkApiError, SdkErrorKind};
use lexe_sha256::sha256;
use reqwest::Url;

use crate::server::RouterState;

/// Hostnames allowed in a request's `Host` header. Rejecting all others
/// prevents DNS rebinding, where a malicious website resolves its own domain
/// to the sidecar's IP to drive the sidecar from the user's browser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllowedHosts {
    /// Allow any `Host`, e.g. when a reverse proxy already validates it.
    Any,
    /// Lowercase hostnames, without ports.
    Only(Vec<String>),
}

impl AllowedHosts {
    /// Additionally allows the loopback hostnames and the host of
    /// `sidecar_url`.
    pub(crate) fn with_defaults(self, sidecar_url: &str) -> Self {
        let Self::Only(mut hosts) = self else {
            return self;
        };

        hosts.extend(["localhost", "127.0.0.1", "[::1]"].map(String::from));
        if let Some(host) = Url::parse(sidecar_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        {
            hosts.push(host);
        }
        Self::Only(hosts)
    }

    fn allows(&self, host: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Only(hosts) =>
                hosts.iter().any(|h| h.eq_ignore_ascii_case(host)),
        }
    }
}

impl Default for AllowedHosts {
    fn default() -> Self {
        Self::Only(Vec::new())
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

pub(crate) fn bearer_token(header: &HeaderValue) -> Result<&str, &'static str> {
    header
        .to_str()
        .map_err(|_| "Auth header contains invalid characters")?
        .strip_prefix("Bearer ")
        .ok_or("Auth header must use 'Bearer ' prefix")
}

/// Rejects requests which may have been sent by a malicious website.
pub(crate) async fn check_access(
    State(state): State<Arc<RouterState>>,
    req: Request,
    next: Next,
) -> Result<Response, SdkApiError> {
    helpers::check_host(&state.allowed_hosts, &req)?;
    helpers::check_origin(state.api_token.as_ref(), &req)?;
    Ok(next.run(req).await)
}

mod helpers {
    use super::*;

    /// Rejects requests whose `Host` isn't in `allowed_hosts`.
    pub(super) fn check_host(
        allowed_hosts: &AllowedHosts,
        req: &Request,
    ) -> Result<(), SdkApiError> {
        // Browsers always send a `Host`, so requests without one are allowed.
        let Some(authority) = request_authority(req)? else {
            return Ok(());
        };
        let host = authority.host();
        if !allowed_hosts.allows(host) {
            return Err(rejection(format!(
                "Host '{host}' is not allowed. To allow it, add it to \
                 --allowed-hosts / $LEXE_SIDECAR_ALLOWED_HOSTS"
            )));
        }
        Ok(())
    }

    /// Requests with an `Origin` header must present the sidecar API token.
    pub(super) fn check_origin(
        api_token: Option<&ApiToken>,
        req: &Request,
    ) -> Result<(), SdkApiError> {
        if !req.headers().contains_key(ORIGIN) {
            return Ok(());
        }

        if let Some(api_token) = api_token
            && let Some(header) = req.headers().get(AUTHORIZATION)
            && let Ok(token) = bearer_token(header)
            && api_token.matches(token)
        {
            return Ok(());
        }

        Err(rejection(
            "Requests with an Origin header require the sidecar API token."
                .to_owned(),
        ))
    }

    /// The request's `:authority` (HTTP/2) or `Host` header (HTTP/1).
    pub(super) fn request_authority(
        req: &Request,
    ) -> Result<Option<Authority>, SdkApiError> {
        if let Some(authority) = req.uri().authority() {
            return Ok(Some(authority.clone()));
        }
        let Some(value) = req.headers().get(HOST) else {
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
}

#[cfg(test)]
mod test {
    use axum::body::Body;

    use super::*;

    #[test]
    fn allowed_hosts() {
        let url = "http://Sidecar.internal:5393";
        let hosts = AllowedHosts::default().with_defaults(url);
        for host in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]"] {
            assert!(hosts.allows(host), "{host}");
        }
        assert!(hosts.allows("sidecar.internal"));
        assert!(!hosts.allows("evil.example"));
        assert!(!hosts.allows("[::2]"));

        let hosts = AllowedHosts::from_str(" Example.com, [::2] ,")
            .unwrap()
            .with_defaults(url);
        assert!(hosts.allows("example.com"));
        assert!(hosts.allows("[::2]"));
        assert!(hosts.allows("localhost"));
        assert!(!hosts.allows("evil.example"));

        let any = AllowedHosts::from_str("example.com,*").unwrap();
        assert_eq!(any, AllowedHosts::Any);
        assert!(any.with_defaults(url).allows("evil.example"));

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
            builder.body(Body::empty()).unwrap()
        };
        let host = |req: &Request| {
            helpers::request_authority(req)
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
        helpers::request_authority(&req("/v2/health", Some("bad/host")))
            .unwrap_err();
    }

    #[test]
    fn api_token() {
        let token = ApiToken::from_str(" s3cret\n").unwrap();
        assert!(token.matches("s3cret"));
        assert!(!token.matches(" s3cret"));
        assert!(!token.matches("s3cre"));
        assert!(ApiToken::from_str(" \n").is_err());
    }

    #[test]
    fn check_origin() {
        let req = |origin: Option<&str>, authorization: Option<&str>| {
            let mut builder = Request::builder().uri("/v2/health");
            if let Some(origin) = origin {
                builder = builder.header(ORIGIN, origin);
            }
            if let Some(authorization) = authorization {
                builder = builder.header(AUTHORIZATION, authorization);
            }
            builder.body(Body::empty()).unwrap()
        };

        let token = ApiToken::from_str("s3cret").unwrap();
        helpers::check_origin(None, &req(None, None)).unwrap();
        helpers::check_origin(Some(&token), &req(None, None)).unwrap();
        for origin in [Some("https://example.com"), Some("null")] {
            helpers::check_origin(None, &req(origin, Some("Bearer s3cret")))
                .unwrap_err();
            for auth in [None, Some("Bearer wrong"), Some("Basic s3cret")] {
                helpers::check_origin(Some(&token), &req(origin, auth))
                    .unwrap_err();
            }
            helpers::check_origin(
                Some(&token),
                &req(origin, Some("Bearer s3cret")),
            )
            .unwrap();
        }
    }
}
