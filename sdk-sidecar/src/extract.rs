use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use axum::extract::FromRequestParts;
use http::{
    HeaderMap,
    header::{AUTHORIZATION, ORIGIN},
};
use lexe::{
    types::auth::{ClientCredentials, Credentials},
    wallet::LexeWallet,
};
use lexe_api::error::SdkApiError;

use crate::{access::ApiToken, server::RouterState};

/// How a request auth'd via its `Authorization: Bearer <token>` header. The
/// token is the sidecar's API token or client credentials.
pub(crate) enum RequestAuth {
    /// Use the default wallet.
    Default,
    /// Use these per-request client credentials.
    ClientCredentials(ClientCredentials),
}

/// Extracts and validates [`RequestAuth`] from the `Authorization` header.
///
/// By default, `Host` and `Origin` header restrictions apply only to unauthed
/// wallet requests, protecting the default wallet from DNS rebinding.
///
/// An explicit Host allowlist applies to every request.
///
/// Expected format: `Authorization: Bearer <credentials>`
pub(crate) struct AuthExtractor(pub RequestAuth);

impl FromRequestParts<Arc<RouterState>> for AuthExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let allows_implicit_auth =
            state.host_policy.allows_implicit_auth(parts)?;
        let api_token = state.api_token.as_ref();
        if let Some(auth) =
            RequestAuth::from_headers(&parts.headers, api_token)?
        {
            return Ok(Self(auth));
        }

        if !allows_implicit_auth || parts.headers.contains_key(ORIGIN) {
            return Err(SdkApiError::bad_auth(
                "Requests with an Origin header or an unlisted Host require \
                 an API token or client credentials in the Authorization header.",
            ));
        }
        if api_token.is_some() {
            return Err(SdkApiError::bad_auth(
                "Using the sidecar's default wallet requires its API token. \
                 Pass it via the header `Authorization: Bearer <api_token>`.",
            ));
        }
        Ok(Self(RequestAuth::Default))
    }
}

impl RequestAuth {
    /// Parses an explicit `Authorization` header, if present.
    ///
    /// The header may contain the configured sidecar [`ApiToken`] or
    /// per-request [`ClientCredentials`].
    pub(crate) fn from_headers(
        headers: &HeaderMap,
        api_token: Option<&ApiToken>,
    ) -> Result<Option<Self>, SdkApiError> {
        let Some(auth_header) = headers.get(AUTHORIZATION) else {
            return Ok(None);
        };

        let auth_str = auth_header.to_str().map_err(|_| {
            SdkApiError::bad_auth("Auth header contains invalid characters")
        })?;
        let token = auth_str.strip_prefix("Bearer ").ok_or_else(|| {
            SdkApiError::bad_auth("Auth header must use 'Bearer ' prefix")
        })?;

        if api_token.is_some_and(|t| t.matches(token)) {
            return Ok(Some(Self::Default));
        }

        let invalid_credentials_msg = match api_token {
            Some(_) =>
                "Bearer token is not the API token and not valid client credentials",
            None => "Client credentials had invalid format",
        };
        let credentials = ClientCredentials::from_str(token)
            .context(invalid_credentials_msg)
            .map_err(SdkApiError::bad_auth)?;

        Ok(Some(Self::ClientCredentials(credentials)))
    }
}

/// Extracts [`Credentials`] and their
/// derived [`LexeWallet`] for the request.
///
/// If any client credentials were found in the `Authorization` header, returns
/// a `LexeWallet` based on those credentials along with the credentials.
/// Otherwise, returns the default wallet configured at startup along with
/// the default credentials. Errors if no credentials are provided and no
/// default wallet exists.
// Logic should be synced with [`LexeWalletExtractor`]
pub(crate) struct WalletAndCredentialsExtractor {
    pub wallet: Arc<LexeWallet>,
    pub credentials: Arc<Credentials>,
}

impl FromRequestParts<Arc<RouterState>> for WalletAndCredentialsExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthExtractor(auth) =
            AuthExtractor::from_request_parts(parts, state).await?;

        let client_credentials = match auth {
            RequestAuth::Default => return Self::from_default(state),
            RequestAuth::ClientCredentials(cc) => cc,
        };

        let wallet = state
            .wallets
            .get(&client_credentials)
            .await
            .map_err(SdkApiError::bad_auth)?;

        let credentials = Credentials::ClientCredentials(client_credentials);

        Ok(Self {
            wallet,
            credentials: Arc::new(credentials),
        })
    }
}

impl WalletAndCredentialsExtractor {
    /// Uses the default wallet, if configured.
    fn from_default(state: &RouterState) -> Result<Self, SdkApiError> {
        let (wallet, credentials) = state
            .wallets
            .default()
            .ok_or(
                "No client credentials configured. \
                 Set LEXE_CLIENT_CREDENTIALS in env or .env, \
                 or pass credentials via the Authorization header.",
            )
            .map_err(SdkApiError::bad_auth)?;

        Ok(Self {
            wallet: wallet.clone(),
            credentials: credentials.clone(),
        })
    }
}

/// Extracts a [`LexeWallet`] for the request.
///
/// If any client credentials were found in the `Authorization` header, returns
/// a `LexeWallet` based on those credentials. Otherwise, returns the default
/// wallet configured at startup. Errors if no credentials are provided and no
/// default wallet exists.
// Logic should be synced with [`LexeWalletAndCredentialsExtractor`]
pub(crate) struct WalletExtractor(pub Arc<LexeWallet>);

impl FromRequestParts<Arc<RouterState>> for WalletExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let extractor =
            WalletAndCredentialsExtractor::from_request_parts(parts, state)
                .await?;

        Ok(Self(extractor.wallet))
    }
}
