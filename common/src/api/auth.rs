// bearer auth v1

use std::{
    fmt,
    time::{Duration, SystemTime},
};

use base64::Engine;
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
    array,
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
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BearerAuthRequest {
    V1(BearerAuthRequestV1),
    // Added in node-v0.7.9+
    V2(BearerAuthRequestV2),
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BearerAuthRequestV1 {
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
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BearerAuthRequestV2 {
    // v2 includes all fields from v1
    pub v1: BearerAuthRequestV1,

    /// The allowed API scope for the bearer auth token.
    pub scope: Option<Scope>,
}

/// The allowed API scope for the bearer auth token.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Scope {
    /// The token is valid for all scopes.
    All,

    /// The token is only allowed to connect to a user node via the gateway.
    // TODO(phlip9): should be a fine-grained scope
    GatewayConnect,
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
pub struct BearerAuthToken(pub ByteStr);

/// A [`BearerAuthToken`] and its expected expiration time
#[derive(Clone)]
pub struct TokenWithExpiration {
    pub expiration: SystemTime,
    pub token: BearerAuthToken,
}

/// A `BearerAuthenticator` (1) stores existing fresh auth tokens and (2)
/// authenticates and fetches new auth tokens when they expire.
pub struct BearerAuthenticator {
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
    pub fn new(now: SystemTime, token_lifetime_secs: u32) -> Self {
        Self::V1(BearerAuthRequestV1 {
            request_timestamp_secs: now
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Something is very wrong with our clock")
                .as_secs(),
            lifetime_secs: token_lifetime_secs,
        })
    }

    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // likewise, user/node auth is (currently) just used to prove ownership
        // of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::UserVerifyError)
    }

    fn v1(&self) -> &BearerAuthRequestV1 {
        match self {
            Self::V1(req) => req,
            Self::V2(req) => &req.v1,
        }
    }

    /// Get the `request_timestamp` as a [`SystemTime`]. Returns `Err` if the
    /// `issued_timestamp` is too large to be represented as a unix timestamp
    /// (> 2^63 on linux).
    pub fn request_timestamp(&self) -> Result<SystemTime, Error> {
        let t_secs = self.v1().request_timestamp_secs;
        let t_dur_secs = Duration::from_secs(t_secs);
        SystemTime::UNIX_EPOCH
            .checked_add(t_dur_secs)
            .ok_or(Error::InvalidTimestamp)
    }

    /// The requested token lifetime in seconds.
    pub fn lifetime_secs(&self) -> u32 {
        self.v1().lifetime_secs
    }

    /// The requested token API scope.
    pub fn scope(&self) -> Option<&Scope> {
        match self {
            Self::V1(_) => None,
            Self::V2(req) => req.scope.as_ref(),
        }
    }
}

impl ed25519::Signable for BearerAuthRequest {
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
        Self {
            user_key_pair,
            cached_auth_token: std::sync::Mutex::new(maybe_token),
            auth_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Read the currently cached and possibly expired (!) bearer auth token.
    ///
    /// This method is only exposed to support the `reqwest::Proxy` workaround
    /// used in `NodeClient`. Try to avoid it otherwise.
    pub fn get_maybe_cached_token(&self) -> Option<TokenWithExpiration> {
        self.cached_auth_token.lock().unwrap().as_ref().cloned()
    }

    /// Try to either (1) return an existing, fresh token or (2) authenticate
    /// with the backend to get a new fresh token (and cache it).
    pub async fn get_token<T: BearerAuthBackendApi + ?Sized>(
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
        let cached_token = self.authenticate(api, now).await?;
        let token_clone = cached_token.token.clone();

        // fill token cache with new token
        *self.cached_auth_token.lock().unwrap() = Some(cached_token);

        Ok(token_clone)
    }

    /// Create a new [`BearerAuthRequest`], sign it, and send the request.
    /// Returns the [`TokenWithExpiration`] if the auth request succeeds.
    ///
    /// NOTE: doesn't update the token cache
    async fn authenticate<T: BearerAuthBackendApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
    ) -> Result<TokenWithExpiration, BackendApiError> {
        let lifetime = DEFAULT_USER_TOKEN_LIFETIME_SECS;
        let expiration = now + Duration::from_secs(lifetime as u64);
        let auth_req = BearerAuthRequest::new(now, lifetime);
        let (_, signed_req) = self
            .user_key_pair
            .sign_struct(&auth_req)
            .map_err(|err| BackendApiError {
                kind: BackendErrorKind::Building,
                msg: format!("Error signing auth request: {err:#}"),
                ..Default::default()
            })?;

        let resp = api.bearer_auth(&signed_req).await?;

        Ok(TokenWithExpiration {
            expiration,
            token: resp.bearer_auth_token,
        })
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
    fn test_bearer_auth_request_canonical() {
        bcs_roundtrip_proptest::<BearerAuthRequest>();
    }

    #[test]
    fn test_bearer_auth_request_sign_verify() {
        signed_roundtrip_proptest::<BearerAuthRequest>();
    }

    #[test]
    fn test_bearer_auth_request_snapshot() {
        let input = "00d20296490000000058020000";
        let req = BearerAuthRequest::V1(BearerAuthRequestV1 {
            request_timestamp_secs: 1234567890,
            lifetime_secs: 10 * 60,
        });
        bcs_roundtrip_ok(&hex::decode(input).unwrap(), &req);

        let input = "01d2029649000000005802000000";
        let req = BearerAuthRequest::V2(BearerAuthRequestV2 {
            v1: BearerAuthRequestV1 {
                request_timestamp_secs: 1234567890,
                lifetime_secs: 10 * 60,
            },
            scope: None,
        });
        bcs_roundtrip_ok(&hex::decode(input).unwrap(), &req);

        let input = "01d202964900000000580200000101";
        let req = BearerAuthRequest::V2(BearerAuthRequestV2 {
            v1: BearerAuthRequestV1 {
                request_timestamp_secs: 1234567890,
                lifetime_secs: 10 * 60,
            },
            scope: Some(Scope::GatewayConnect),
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
        let scope = Scope::GatewayConnect;
        bcs_roundtrip_ok(input, &scope);
    }
}
