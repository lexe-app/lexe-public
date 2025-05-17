// bearer auth v1

use std::{
    fmt,
    time::{Duration, SystemTime},
};

use base64::Engine;
use lexe_std::array;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::user::NodePkProof;
#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    api::{
        def::BearerAuthBackendApi,
        error::{BackendApiError, BackendErrorKind},
    },
    byte_str::ByteStr,
    ed25519::{self, Signed},
};

pub const DEFAULT_USER_TOKEN_LIFETIME_SECS: u32 = 10 * 60; // 10 min
/// The min remaining lifetime of a token before we'll proactively refresh.
const EXPIRATION_BUFFER: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum Error {
    #[error("error verifying signed bearer auth request: {0}")]
    UserVerifyError(#[source] ed25519::Error),

    #[error("Decoded bearer auth token appears malformed")]
    MalformedToken,

    #[error("issued timestamp is too far from current auth server clock")]
    ClockDrift,

    #[error("auth token or auth request is expired")]
    Expired,

    #[error("timestamp is not a valid unix timestamp")]
    InvalidTimestamp,

    #[error("requested token lifetime is too long")]
    InvalidLifetime,

    #[error("user not signed up yet")]
    NoUser,

    #[error("bearer auth token is not valid base64")]
    Base64Decode,

    #[error("bearer auth token was not provided")]
    Missing,

    // TODO(phlip9): this is an authorization error, not an authentication
    // error. Add a new type?
    #[error(
        "auth token's granted scope ({granted:?}) is not sufficient for \
         requested scope ({requested:?})"
    )]
    InsufficientScope { granted: Scope, requested: Scope },
}

/// The inner, signed part of the request a new user makes when they first sign
/// up. We use this to prove the user owns both their claimed [`UserPk`] and
/// [`NodePk`].
///
/// One caveat: we can't verify the presented, valid, signed [`UserPk`] and
/// [`NodePk`] are actually derived from the same [`RootSeed`]. In the case that
/// these are different, the account will be created, but the user node will
/// fail to ever run or provision.
///
/// [`UserPk`]: crate::api::user::UserPk
/// [`NodePk`]: crate::api::user::NodePk
/// [`RootSeed`]: crate::root_seed::RootSeed
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserSignupRequest {
    /// The lightning node pubkey in a Proof-of-Key-Possession
    pub node_pk_proof: NodePkProof,

    /// The user's signup code, if provided.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    pub signup_code: Option<String>,
    // do we need this?
    // pub display_name: Option<String>,

    // probably collect this later in a settings screen. would need email
    // verify flow. can also just not collect email at all and send push
    // notifs only. pub email: Option<String>,

    // TODO(phlip9): other fields? region? language?
}

/// A client's request for a new [`BearerAuthToken`].
///
/// This is the convenient in-memory representation.
#[derive(Clone, Debug)]
pub struct BearerAuthRequest {
    /// The timestamp of this auth request, in seconds since UTC Unix time,
    /// interpreted relative to the server clock. Used to prevent replaying old
    /// auth requests after the ~1 min expiration.
    ///
    /// The server will reject timestamps w/ > 1 minute clock skew from the
    /// server clock.
    pub request_timestamp_secs: u64,

    /// How long the new auth token should be valid for, in seconds. Must be at
    /// most 1 hour. The new token expiration is generated relative to the
    /// server clock.
    pub lifetime_secs: u32,

    /// The allowed API scope for the bearer auth token. If unset, the issued
    /// token currently defaults to [`Scope::All`].
    // TODO(phlip9): implement proper scope attenuation from identity's allowed
    // scopes
    pub scope: Option<Scope>,
}

/// A client's request for a new [`BearerAuthToken`].
///
/// This is the over-the-wire BCS-serializable representation structured for
/// backwards compatibility.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BearerAuthRequestWire {
    V1(BearerAuthRequestWireV1),
    // Added in node-v0.7.9+
    V2(BearerAuthRequestWireV2),
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BearerAuthRequestWireV1 {
    request_timestamp_secs: u64,
    lifetime_secs: u32,
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BearerAuthRequestWireV2 {
    // v2 includes all fields from v1
    v1: BearerAuthRequestWireV1,
    scope: Option<Scope>,
}

/// The allowed API scope for the bearer auth token.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Scope {
    /// The token is valid for all scopes.
    All,

    /// The token is only allowed to connect to a user node via the gateway.
    // TODO(phlip9): should be a fine-grained scope
    NodeConnect,
    //
    // // TODO(phlip9): fine-grained scopes?
    // Restricted { .. },
    // ReadOnly,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BearerAuthResponse {
    pub bearer_auth_token: BearerAuthToken,
}

/// An opaque bearer auth token for authenticating user clients against lexe
/// infra as a particular [`UserPk`](crate::api::user::UserPk).
///
/// Most user clients should just treat this as an opaque Bearer token with a
/// very short (~15 min) expiration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq))]
pub struct BearerAuthToken(pub ByteStr);

/// A [`BearerAuthToken`] and its expected expiration time
#[derive(Clone)]
pub struct TokenWithExpiration {
    pub expiration: SystemTime,
    pub token: BearerAuthToken,
}

/// A `BearerAuthenticator` (1) stores existing fresh auth tokens and (2)
/// authenticates and fetches new auth tokens when they expire.
#[allow(private_interfaces)]
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
    /// [`UserPk`]: crate::api::user::UserPk
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

// -- impl UserSignupRequest -- //

impl UserSignupRequest {
    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // for user sign up, the signed signup request is just used to prove
        // ownership of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::UserVerifyError)
    }
}

impl ed25519::Signable for UserSignupRequest {
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::UserSignupRequest");
}

// -- impl BearerAuthRequest -- //

impl BearerAuthRequest {
    pub fn new(
        now: SystemTime,
        token_lifetime_secs: u32,
        scope: Option<Scope>,
    ) -> Self {
        Self {
            request_timestamp_secs: now
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Something is very wrong with our clock")
                .as_secs(),
            lifetime_secs: token_lifetime_secs,
            scope,
        }
    }

    /// Get the `request_timestamp` as a [`SystemTime`]. Returns `Err` if the
    /// `issued_timestamp` is too large to be represented as a unix timestamp
    /// (> 2^63 on linux).
    pub fn request_timestamp(&self) -> Result<SystemTime, Error> {
        let t_secs = self.request_timestamp_secs;
        let t_dur_secs = Duration::from_secs(t_secs);
        SystemTime::UNIX_EPOCH
            .checked_add(t_dur_secs)
            .ok_or(Error::InvalidTimestamp)
    }
}

impl From<BearerAuthRequestWire> for BearerAuthRequest {
    fn from(wire: BearerAuthRequestWire) -> Self {
        match wire {
            BearerAuthRequestWire::V1(v1) => Self {
                request_timestamp_secs: v1.request_timestamp_secs,
                lifetime_secs: v1.lifetime_secs,
                scope: None,
            },
            BearerAuthRequestWire::V2(v2) => Self {
                request_timestamp_secs: v2.v1.request_timestamp_secs,
                lifetime_secs: v2.v1.lifetime_secs,
                scope: v2.scope,
            },
        }
    }
}

impl From<BearerAuthRequest> for BearerAuthRequestWire {
    fn from(req: BearerAuthRequest) -> Self {
        Self::V2(BearerAuthRequestWireV2 {
            v1: BearerAuthRequestWireV1 {
                request_timestamp_secs: req.request_timestamp_secs,
                lifetime_secs: req.lifetime_secs,
            },
            scope: req.scope,
        })
    }
}

// -- impl BearerAuthRequestWire -- //

impl BearerAuthRequestWire {
    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // likewise, user/node auth is (currently) just used to prove ownership
        // of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::UserVerifyError)
    }
}

impl ed25519::Signable for BearerAuthRequestWire {
    // Uses "LEXE-REALM::BearerAuthRequest" for backwards compatibility
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::BearerAuthRequest");
}

// -- impl BearerAuthToken -- //

impl BearerAuthToken {
    /// base64 serialize a bearer auth token from the internal raw bytes.
    pub fn encode_from_raw_bytes(signed_token_bytes: &[u8]) -> Self {
        let b64_token = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signed_token_bytes);
        Self(ByteStr::from(b64_token))
    }

    /// base64 decode the bearer auth token into the internal raw bytes.
    pub fn decode_into_raw_bytes(&self) -> Result<Vec<u8>, Error> {
        Self::decode_inner_into_raw_bytes(self.0.as_bytes())
    }

    /// base64 decode a string bearer auth token into the internal raw bytes.
    pub fn decode_inner_into_raw_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(bytes)
            .map_err(|_| Error::Base64Decode)
    }
}

impl fmt::Display for BearerAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
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

// --- impl Scope --- //

impl Scope {
    /// Returns `true` if the `requested_scope` is allowed by this granted
    /// scope.
    pub fn has_permission_for(&self, requested_scope: &Self) -> bool {
        let granted_scope = self;
        match (granted_scope, requested_scope) {
            (Scope::All, _) => true,
            (Scope::NodeConnect, Scope::All) => false,
            (Scope::NodeConnect, Scope::NodeConnect) => true,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip::{
        bcs_roundtrip_ok, bcs_roundtrip_proptest, signed_roundtrip_proptest,
    };

    #[test]
    fn test_user_signup_request_canonical() {
        bcs_roundtrip_proptest::<UserSignupRequest>();
    }

    #[test]
    fn test_user_signed_request_sign_verify() {
        signed_roundtrip_proptest::<UserSignupRequest>();
    }

    #[test]
    fn test_bearer_auth_request_wire_canonical() {
        bcs_roundtrip_proptest::<BearerAuthRequestWire>();
    }

    #[test]
    fn test_bearer_auth_request_wire_sign_verify() {
        signed_roundtrip_proptest::<BearerAuthRequestWire>();
    }

    #[test]
    fn test_bearer_auth_request_wire_snapshot() {
        let input = "00d20296490000000058020000";
        let req = BearerAuthRequestWire::V1(BearerAuthRequestWireV1 {
            request_timestamp_secs: 1234567890,
            lifetime_secs: 10 * 60,
        });
        bcs_roundtrip_ok(&hex::decode(input).unwrap(), &req);

        let input = "01d2029649000000005802000000";
        let req = BearerAuthRequestWire::V2(BearerAuthRequestWireV2 {
            v1: BearerAuthRequestWireV1 {
                request_timestamp_secs: 1234567890,
                lifetime_secs: 10 * 60,
            },
            scope: None,
        });
        bcs_roundtrip_ok(&hex::decode(input).unwrap(), &req);

        let input = "01d202964900000000580200000101";
        let req = BearerAuthRequestWire::V2(BearerAuthRequestWireV2 {
            v1: BearerAuthRequestWireV1 {
                request_timestamp_secs: 1234567890,
                lifetime_secs: 10 * 60,
            },
            scope: Some(Scope::NodeConnect),
        });
        bcs_roundtrip_ok(&hex::decode(input).unwrap(), &req);
    }

    #[test]
    fn test_auth_scope_canonical() {
        bcs_roundtrip_proptest::<Scope>();
    }

    #[test]
    fn test_auth_scope_snapshot() {
        let input = b"\x00";
        let scope = Scope::All;
        bcs_roundtrip_ok(input, &scope);

        let input = b"\x01";
        let scope = Scope::NodeConnect;
        bcs_roundtrip_ok(input, &scope);
    }
}
