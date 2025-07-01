use std::time::{Duration, SystemTime};

use common::{
    api::auth::{
        BearerAuthRequest, BearerAuthRequestWire, BearerAuthToken, Scope,
        TokenWithExpiration,
    },
    ed25519::{self},
};
use lexe_api_core::error::{BackendApiError, BackendErrorKind};

use crate::def::BearerAuthBackendApi;

pub const DEFAULT_USER_TOKEN_LIFETIME_SECS: u32 = 10 * 60; // 10 min
/// The min remaining lifetime of a token before we'll proactively refresh.
const EXPIRATION_BUFFER: Duration = Duration::from_secs(30);

/// A `BearerAuthenticator` (1) stores existing fresh auth tokens and (2)
/// authenticates and fetches new auth tokens when they expire.
#[allow(private_interfaces, clippy::large_enum_variant)]
pub enum BearerAuthenticator {
    Ephemeral { inner: EphemeralBearerAuthenticator },
    Static { inner: StaticBearerAuthenticator },
}

/// Our standard [`BearerAuthenticator`] that re-authenticates and requests a
/// new short-lived, ephemeral token every ~10 minutes.
struct EphemeralBearerAuthenticator {
    /// The [`ed25519::KeyPair`] for the [`UserPk`], used to authenticate with
    /// the lexe backend.
    ///
    /// [`UserPk`]: common::api::user::UserPk
    user_key_pair: ed25519::KeyPair,

    /// The latest [`BearerAuthToken`] with its expected expiration time.
    // Ideally the `Option<TokenWithExpiration>` would live in the `auth_lock`
    // (as it did previously); however, we need to read the latest cached
    // version from a blocking context in the `NodeClient` proxy auth
    // workaround
    cached_auth_token: std::sync::Mutex<Option<TokenWithExpiration>>,

    /// A `tokio` mutex to ensure that only one task can auth at a time, if
    /// multiple tasks are racing to auth at the same time.
    // NOTE: we intenionally use a tokio async `Mutex` here:
    //
    // 1. we want only at-most-one client to try auth'ing at once
    // 2. auth'ing involves IO (send/recv HTTPS request)
    // 3. holding a standard blocking `Mutex` across IO await points is a Bad
    //    Idea^tm, since it'll block all tasks on the runtime (we only use a
    //    single thread for the user node).
    auth_lock: tokio::sync::Mutex<()>,

    /// The API scope this authenticator will request for its auth tokens.
    scope: Option<Scope>,
}

// TODO(phlip9): we should be able to remove this once we have proper delegated
// identities that can request bearer auth tokens themselves _for_ a `UserPk`.
struct StaticBearerAuthenticator {
    /// The fixed, long-lived auth token.
    token: BearerAuthToken,
}

// --- impl BearerAuthenticator --- //

impl BearerAuthenticator {
    /// Create a new `BearerAuthenticator` with the auth `api` handle, the
    /// `user_key_pair` (for signing auth requests), and an optional existing
    /// token.
    pub fn new(
        user_key_pair: ed25519::KeyPair,
        maybe_token: Option<TokenWithExpiration>,
    ) -> Self {
        // Use default scope for this identity.
        let scope = None;
        Self::new_with_scope(user_key_pair, maybe_token, scope)
    }

    /// [`BearerAuthenticator::new`] constructor with an optional scope to
    /// restrict requested auth tokens.
    pub fn new_with_scope(
        user_key_pair: ed25519::KeyPair,
        maybe_token: Option<TokenWithExpiration>,
        scope: Option<Scope>,
    ) -> Self {
        Self::Ephemeral {
            inner: EphemeralBearerAuthenticator {
                user_key_pair,
                cached_auth_token: std::sync::Mutex::new(maybe_token),
                auth_lock: tokio::sync::Mutex::new(()),
                scope,
            },
        }
    }

    /// A [`BearerAuthenticator`] that always returns the same static,
    /// long-lived token.
    pub fn new_static_token(token: BearerAuthToken) -> Self {
        Self::Static {
            inner: StaticBearerAuthenticator { token },
        }
    }

    pub fn user_key_pair(&self) -> Option<&ed25519::KeyPair> {
        match self {
            Self::Ephemeral { inner } => Some(&inner.user_key_pair),
            Self::Static { .. } => None,
        }
    }

    /// Read the currently cached and possibly expired (!) bearer auth token.
    ///
    /// This method is only exposed to support the `reqwest::Proxy` workaround
    /// used in `NodeClient`. Try to avoid it otherwise.
    pub fn get_maybe_cached_token(&self) -> Option<BearerAuthToken> {
        match self {
            Self::Ephemeral { inner } => inner.get_maybe_cached_token(),
            Self::Static { inner } => inner.get_maybe_cached_token(),
        }
    }

    /// Try to either (1) return an existing, fresh token or (2) authenticate
    /// with the backend to get a new fresh token (and cache it).
    pub async fn get_token<T: BearerAuthBackendApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
    ) -> Result<BearerAuthToken, BackendApiError> {
        match self {
            Self::Ephemeral { inner } => inner.get_token(api, now).await,
            Self::Static { inner } => inner.get_token(api, now).await,
        }
    }
}

impl EphemeralBearerAuthenticator {
    fn get_maybe_cached_token(&self) -> Option<BearerAuthToken> {
        self.cached_auth_token
            .lock()
            .unwrap()
            .as_ref()
            .map(|token_with_exp| token_with_exp.token.clone())
    }

    /// Try to either (1) return an existing, fresh token or (2) authenticate
    /// with the backend to get a new fresh token (and cache it).
    async fn get_token<T: BearerAuthBackendApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
    ) -> Result<BearerAuthToken, BackendApiError> {
        let _auth_lock = self.auth_lock.lock().await;

        // there's already a fresh token here; just use that.
        if let Some(cached_token) =
            self.cached_auth_token.lock().unwrap().as_ref()
        {
            // Buffer ensures we don't return immediately expiring tokens
            if now + EXPIRATION_BUFFER < cached_token.expiration {
                return Ok(cached_token.token.clone());
            }
        }

        // no token yet or expired, try to authenticate and get a new token.
        let token_with_exp = do_bearer_auth(
            api,
            now,
            &self.user_key_pair,
            DEFAULT_USER_TOKEN_LIFETIME_SECS,
            self.scope.clone(),
        )
        .await?;

        let token_clone = token_with_exp.token.clone();

        // fill token cache with new token
        *self.cached_auth_token.lock().unwrap() = Some(token_with_exp);

        Ok(token_clone)
    }
}

impl StaticBearerAuthenticator {
    fn get_maybe_cached_token(&self) -> Option<BearerAuthToken> {
        Some(self.token.clone())
    }

    async fn get_token<T: BearerAuthBackendApi + ?Sized>(
        &self,
        _api: &T,
        _now: SystemTime,
    ) -> Result<BearerAuthToken, BackendApiError> {
        Ok(self.token.clone())
    }
}

// --- Helpers --- //

/// Create a new short-lived [`BearerAuthRequest`], sign it, and send the
/// request. Returns the [`TokenWithExpiration`] if the auth request
/// succeeds.
pub async fn do_bearer_auth<T: BearerAuthBackendApi + ?Sized>(
    api: &T,
    now: SystemTime,
    user_key_pair: &ed25519::KeyPair,
    lifetime_secs: u32,
    scope: Option<Scope>,
) -> Result<TokenWithExpiration, BackendApiError> {
    let expiration = now + Duration::from_secs(lifetime_secs as u64);
    let auth_req = BearerAuthRequest::new(now, lifetime_secs, scope);
    let auth_req_wire = BearerAuthRequestWire::from(auth_req);
    let (_, signed_req) =
        user_key_pair.sign_struct(&auth_req_wire).map_err(|err| {
            BackendApiError {
                kind: BackendErrorKind::Building,
                msg: format!("Error signing auth request: {err:#}"),
                ..Default::default()
            }
        })?;

    let resp = api.bearer_auth(&signed_req).await?;

    Ok(TokenWithExpiration {
        expiration,
        token: resp.bearer_auth_token,
    })
}
