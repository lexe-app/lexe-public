//! Client-side bearer auth: requesting, caching, and presenting the
//! `BearerAuthToken`s used to authenticate against Lexe-run services.
//!
//! A `BearerAuthenticator` holds the credential a client authenticates with and
//! hands out fresh tokens on demand, scoped by `LexeScope`:
//!
//! - `Ephemeral` holds a user key pair and mints a fresh, short-lived token
//!   whenever one is needed, caching the latest token *per scope* so repeated
//!   calls are cheap.
//! - `Static` holds a single pre-minted, long-lived token of a fixed scope
//!   (e.g. a client credential's `GatewayProxy` token); it can serve any
//!   request whose scope it covers, but cannot mint new tokens.

use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use lexe_api_core::error::{BackendApiError, BackendErrorKind};
use lexe_common::api::auth::{
    BearerAuthRequest, BearerAuthRequestWire, BearerAuthToken, LexeScope,
    TokenWithExpiration,
};
use lexe_crypto::ed25519;

use crate::def::BearerAuthBackendApi;

pub const DEFAULT_USER_TOKEN_LIFETIME_SECS: u32 = 10 * 60; // 10 min
/// The min remaining lifetime of a token before we'll proactively refresh.
const EXPIRATION_BUFFER: Duration = Duration::from_secs(30);

/// Hands out fresh [`BearerAuthToken`]s for a given [`LexeScope`], caching them
/// until they expire. See the [module docs](self) for the two variants.
#[allow(clippy::large_enum_variant)]
pub enum BearerAuthenticator {
    /// Our standard authenticator, which mints a fresh short-lived token
    /// whenever the cached one for a given scope expires.
    Ephemeral {
        /// The [`ed25519::KeyPair`] for the [`UserPk`], used to authenticate
        /// with the lexe backend.
        ///
        /// [`UserPk`]: lexe_common::api::user::UserPk
        user_key_pair: ed25519::KeyPair,

        /// The latest fresh token for each [`LexeScope`] we've requested,
        /// behind a `tokio` mutex so that at most one task auths at a time.
        // NOTE: we intentionally use a tokio async `Mutex` here:
        //
        // 1. we want only at-most-one client to try auth'ing at once
        // 2. auth'ing involves IO (send/recv HTTPS request)
        // 3. holding a standard blocking `Mutex` across IO await points is a
        //    Bad Idea^tm, since it'll block all tasks on the runtime (we only
        //    use a single thread for the user node).
        cache: tokio::sync::Mutex<HashMap<LexeScope, TokenWithExpiration>>,
    },

    /// A single pre-minted, long-lived token of a fixed scope, which can serve
    /// any request whose scope it covers but cannot mint new tokens.
    // TODO(phlip9): we should be able to remove this once we have proper
    // delegated identities that can request bearer auth tokens themselves
    // _for_ a `UserPk`.
    Static {
        /// The fixed, long-lived auth token.
        token: BearerAuthToken,
        /// The scope that was passed in alongside the token when this
        /// [`BearerAuthenticator`] was created, i.e. the scope the caller
        /// *expects* was granted to the token. The actual authorization scope
        /// is ultimately determined by the verifying server.
        scope: LexeScope,
    },
}

// --- impl BearerAuthenticator --- //

impl BearerAuthenticator {
    /// Create an [`Ephemeral`](Self::Ephemeral) authenticator that signs auth
    /// requests with `user_key_pair` and mints tokens on demand.
    pub fn new(user_key_pair: ed25519::KeyPair) -> Self {
        Self::Ephemeral {
            user_key_pair,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Create a [`Static`](Self::Static) authenticator that always returns the
    /// same long-lived `token`. Pass the `scope` that the `token` was granted.
    pub fn new_static_token(token: BearerAuthToken, scope: LexeScope) -> Self {
        Self::Static { token, scope }
    }

    pub fn user_key_pair(&self) -> Option<&ed25519::KeyPair> {
        match self {
            Self::Ephemeral { user_key_pair, .. } => Some(user_key_pair),
            Self::Static { .. } => None,
        }
    }

    /// Get a fresh token for the requested `scope`: either a still-fresh cached
    /// token, or a newly authenticated one (which is then cached).
    pub async fn get_token<T: BearerAuthBackendApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
        scope: LexeScope,
    ) -> Result<BearerAuthToken, BackendApiError> {
        self.get_token_with_exp(api, now, scope)
            .await
            .map(|token_with_exp| token_with_exp.token)
    }

    /// [`get_token`](Self::get_token), but also returns the token's expiration.
    pub async fn get_token_with_exp<T: BearerAuthBackendApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
        scope: LexeScope,
    ) -> Result<TokenWithExpiration, BackendApiError> {
        match self {
            Self::Ephemeral {
                user_key_pair,
                cache,
            } => {
                let mut cache = cache.lock().await;

                // There's already a fresh token for this scope; just use that.
                if let Some(token) = cache.get(&scope)
                    && !helpers::token_needs_refresh(now, token.expiration)
                {
                    return Ok(token.clone());
                }

                // No token yet or expired; authenticate and cache a new one.
                let token_with_exp = helpers::do_bearer_auth(
                    api,
                    now,
                    user_key_pair,
                    DEFAULT_USER_TOKEN_LIFETIME_SECS,
                    scope,
                )
                .await?;
                cache.insert(scope, token_with_exp.clone());

                Ok(token_with_exp)
            }
            Self::Static {
                token,
                scope: granted,
            } => {
                if !granted.has_permission_for(&scope) {
                    return Err(BackendApiError {
                        kind: BackendErrorKind::Unauthorized,
                        msg: format!(
                            "Static auth token's scope ({granted:?}) does not \
                             cover the requested scope ({scope:?})"
                        ),
                        ..Default::default()
                    });
                }

                Ok(TokenWithExpiration {
                    expiration: None,
                    token: token.clone(),
                })
            }
        }
    }
}

/// Bearer auth helpers.
pub mod helpers {
    use super::*;

    /// Create a new [`BearerAuthRequest`], sign it, and send it. Returns the
    /// [`TokenWithExpiration`] if the auth request succeeds.
    pub async fn do_bearer_auth<T: BearerAuthBackendApi + ?Sized>(
        api: &T,
        now: SystemTime,
        user_key_pair: &ed25519::KeyPair,
        lifetime_secs: u32,
        scope: LexeScope,
    ) -> Result<TokenWithExpiration, BackendApiError> {
        let expiration = now + Duration::from_secs(lifetime_secs as u64);
        let auth_req = BearerAuthRequest::new(now, lifetime_secs, scope);
        let auth_req_wire = BearerAuthRequestWire::from(auth_req);
        let (_, signed_req) = user_key_pair
            .sign_struct(&auth_req_wire)
            .map_err(|err| BackendApiError {
                kind: BackendErrorKind::Building,
                msg: format!("Error signing auth request: {err:#}"),
                ..Default::default()
            })?;

        let resp = api.bearer_auth(&signed_req).await?;

        Ok(TokenWithExpiration {
            expiration: Some(expiration),
            token: resp.bearer_auth_token,
        })
    }

    /// Returns `true` if the token is expired or about to expire.
    #[inline]
    pub fn token_needs_refresh(
        now: SystemTime,
        expiration: Option<SystemTime>,
    ) -> bool {
        // Buffer ensures we don't return immediately expiring tokens.
        match expiration {
            Some(expiration) => now + EXPIRATION_BUFFER >= expiration,
            None => false,
        }
    }
}
