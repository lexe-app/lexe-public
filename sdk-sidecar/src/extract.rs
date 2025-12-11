use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use axum::extract::FromRequestParts;
use common::{ed25519, rng::SysRng};
use http::header::AUTHORIZATION;
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::{ClientCredentials, Credentials},
};
use sdk_core::SdkApiError;

use crate::server::RouterState;

/// The user agent string for internal requests.
static USER_AGENT_INTERNAL: &str = lexe_api::user_agent_internal!();

/// Extracts and validates [`ClientCredentials`] from `Authorization` header.
///
/// Returns `None` if the header is not present.
/// Returns an error if the header is present but invalid.
///
/// Expected format: `Authorization: Bearer <credentials>`
pub(crate) struct CredentialsExtractor(pub Option<ClientCredentials>);

impl<S> FromRequestParts<S> for CredentialsExtractor
where
    S: Send + Sync,
{
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
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

        // Verify that the client pubkey derived from the client keypair
        // matches the client public key in `ClientCredentials`.
        let rev_client_keypair = ed25519::KeyPair::deserialize_pkcs8_der(
            credentials.rev_client_key_der.as_bytes(),
        )
        .map_err(|_| "Client key is invalid or corrupted")
        .map_err(SdkApiError::bad_auth)?;

        if rev_client_keypair.public_key() != &credentials.client_pk {
            return Err(SdkApiError::bad_auth(
                "Client key does not match client public key",
            ));
        }

        Ok(Self(Some(credentials)))
    }
}

/// Extracts a [`NodeClient`] for the request.
///
/// If any client credentials were found in the `Authorization` header, returns
/// `NodeClient` based on those credentials.
/// Otherwise, returns the default client configured at startup.
/// Errors if no credentials are provided and no default client exists.
pub(crate) struct NodeClientExtractor(pub NodeClient);

impl FromRequestParts<Arc<RouterState>> for NodeClientExtractor {
    type Rejection = SdkApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &Arc<RouterState>,
    ) -> Result<Self, Self::Rejection> {
        let maybe_credentials =
            CredentialsExtractor::from_request_parts(parts, state).await?;

        if let Some(credentials) = maybe_credentials.0 {
            let client_pk = credentials.client_pk;
            let mut locked_cache = state.client_cache.lock().unwrap();

            let client = match locked_cache.get(&client_pk) {
                Some(c) => c,
                None => {
                    // Create new client and insert into cache

                    let gateway_client = GatewayClient::new(
                        state.deploy_env,
                        state.gateway_url.clone(),
                        USER_AGENT_INTERNAL,
                    )
                    .context("Failed to create gateway client")
                    .map_err(SdkApiError::bad_auth)?;

                    let credentials =
                        Credentials::from_client_credentials(&credentials);

                    let mut rng = SysRng::new();
                    let use_sgx = true;

                    let client = NodeClient::new(
                        &mut rng,
                        use_sgx,
                        state.deploy_env,
                        gateway_client,
                        credentials,
                    )
                    .context("Failed to create node client")
                    .map_err(SdkApiError::bad_auth)?;

                    locked_cache.insert(client_pk, client);
                    locked_cache.get(&client_pk).unwrap()
                }
            };

            return Ok(Self(client.clone()));
        }

        // Fall back to the default client if available, otherwise error.
        let client = state
            .default_client
            .clone()
            .ok_or(
                "No client credentials configured. \
                 Set LEXE_CLIENT_CREDENTIALS in env or .env, \
                 or pass credentials via the Authorization header.",
            )
            .map_err(SdkApiError::bad_auth)?;

        Ok(Self(client))
    }
}
