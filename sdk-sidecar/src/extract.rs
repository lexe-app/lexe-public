use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use axum::extract::FromRequestParts;
use http::header::AUTHORIZATION;
use lexe::{
    types::auth::{ClientCredentials, Credentials, CredentialsRef},
    wallet::LexeWallet,
};
use lexe_api::error::SdkApiError;

use crate::server::RouterState;

/// Extracts and validates [`ClientCredentials`] from `Authorization` header.
///
/// Returns `None` if the header is not present.
/// Returns an error if the header is present but invalid.
///
/// Expected format: `Authorization: Bearer <credentials>`
pub(crate) struct CredentialsExtractor(pub Option<ClientCredentials>);

impl FromRequestParts<Arc<RouterState>> for CredentialsExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(auth_header) = parts.headers.get(AUTHORIZATION) else {
            return Ok(Self(None));
        };

        let auth_str = auth_header
            .to_str()
            .map_err(|_| "Auth header contains invalid characters")
            .map_err(SdkApiError::bad_auth)?;

        // Be strict about the format: AIs calling this API should know the
        // correct format, and leniency could mask security issues.
        let credentials_str = auth_str
            .strip_prefix("Bearer ")
            .ok_or("Auth header must use 'Bearer ' prefix")
            .map_err(SdkApiError::bad_auth)?;

        // Parse credentials
        let credentials = ClientCredentials::from_str(credentials_str)
            .context("Client credentials had invalid format")
            .map_err(SdkApiError::bad_auth)?;

        Ok(Self(Some(credentials)))
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
        let maybe_credentials =
            CredentialsExtractor::from_request_parts(parts, state).await?;

        if let Some(client_credentials) = maybe_credentials.0 {
            let client_pk = client_credentials.unstable().client_pk;
            let mut locked_cache = state.wallet_cache.lock().unwrap();

            // Check cache or create new
            let wallet = match locked_cache.get(&client_pk) {
                Some(cached_wallet) => cached_wallet.clone(),
                None => {
                    let credentials_ref =
                        CredentialsRef::from(&client_credentials);

                    // Create new wallet and insert into cache
                    let wallet = LexeWallet::without_db(
                        state.wallet_env_config.clone(),
                        credentials_ref,
                    )
                    .context("Failed to create wallet")
                    .map_err(SdkApiError::bad_auth)?;

                    let arc_wallet = Arc::new(wallet);

                    locked_cache.insert(client_pk, arc_wallet.clone());
                    arc_wallet
                }
            };

            let credentials =
                Credentials::ClientCredentials(client_credentials);

            return Ok(Self {
                wallet,
                credentials: Arc::new(credentials),
            });
        }

        // Fall back to the default wallet if available, otherwise error.
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
