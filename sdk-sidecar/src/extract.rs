use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use axum::extract::FromRequestParts;
use http::header::AUTHORIZATION;
use lexe::{
    types::auth::{ClientCredentials, Credentials, CredentialsRef},
    wallet::LexeWallet,
};
use lexe_api::error::SdkApiError;

use crate::{access::bearer_token, server::RouterState};

/// How a request authenticates, per its `Authorization: Bearer <token>`
/// header, where the token is the sidecar's API token or client credentials.
pub(crate) enum RequestAuth {
    /// Use the default wallet.
    Default,
    /// The request may not use the default wallet, since it lacks the
    /// required API token.
    MissingApiToken,
    /// Use these per-request client credentials.
    ClientCredentials(ClientCredentials),
}

/// Extracts and validates the [`RequestAuth`] from the `Authorization` header.
pub(crate) struct AuthExtractor(pub RequestAuth);

impl FromRequestParts<Arc<RouterState>> for AuthExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(auth_header) = parts.headers.get(AUTHORIZATION) else {
            return Ok(Self(match state.api_token {
                Some(_) => RequestAuth::MissingApiToken,
                None => RequestAuth::Default,
            }));
        };

        // Be strict about the format: AIs calling this API should know the
        // correct format, and leniency could mask security issues.
        let token = bearer_token(auth_header).map_err(SdkApiError::bad_auth)?;

        if state.api_token.as_ref().is_some_and(|t| t.matches(token)) {
            return Ok(Self(RequestAuth::Default));
        }

        let invalid_credentials_msg = match state.api_token {
            Some(_) =>
                "Bearer token is not the API token and not valid client credentials",
            None => "Client credentials had invalid format",
        };
        let credentials = ClientCredentials::from_str(token)
            .context(invalid_credentials_msg)
            .map_err(SdkApiError::bad_auth)?;

        Ok(Self(RequestAuth::ClientCredentials(credentials)))
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
            RequestAuth::MissingApiToken =>
                return Err(helpers::missing_api_token()),
            RequestAuth::ClientCredentials(cc) => cc,
        };

        let client_pk = client_credentials.unstable().client_pk;
        let mut locked_cache = state.wallet_cache.lock().unwrap();

        // Check cache or create new
        let wallet = match locked_cache.get(&client_pk) {
            Some(cached_wallet) => cached_wallet.clone(),
            None => {
                let credentials_ref = CredentialsRef::from(&client_credentials);

                // Create new wallet and insert into cache
                let wallet = LexeWallet::load_or_fresh(
                    state.wallet_env_config.clone(),
                    credentials_ref,
                    Some(state.data_dir.clone()),
                )
                .context("Failed to create wallet")
                .map_err(SdkApiError::bad_auth)?;

                let arc_wallet = Arc::new(wallet);

                locked_cache.insert(client_pk, arc_wallet.clone());
                arc_wallet
            }
        };

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
            .default
            .as_ref()
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

pub(crate) mod helpers {
    use super::*;

    pub(crate) fn missing_api_token() -> SdkApiError {
        SdkApiError::bad_auth(
            "Using the sidecar's default wallet requires its API token. \
             Pass it via the header `Authorization: Bearer <api_token>`.",
        )
    }
}
