// user auth v1

use std::fmt;
use std::time::{Duration, SystemTime};

#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::def::UserAuthApi;
use crate::api::error::BackendApiError;
use crate::byte_str::ByteStr;
use crate::ed25519::{self, Signed};

pub const DEFAULT_USER_TOKEN_LIFETIME_SECS: u32 = 10 * 60; // 10 min

#[derive(Debug, Error)]
pub enum Error {
    #[error("error verifying user signed struct: {0}")]
    VerifyError(#[from] ed25519::Error),

    #[error("issued timestamp is too far from current auth server clock")]
    ClockDrift,

    #[error("timestamp is not a valid unix timestamp")]
    InvalidTimestamp,

    #[error("requested token lifetime is too long")]
    InvalidLifetime,

    #[error("user not signed up yet")]
    NoUser,

    #[error("user auth token is not valid base64")]
    Base64Decode,
}

#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum UserSignupRequest {
    V1(UserSignupRequestV1),
}

// TODO(phlip9): do we even need any signup fields?
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct UserSignupRequestV1 {
    // do we need this?
    // pub display_name: Option<String>,

    // probably collect this later in a settings screen. would need email verify
    // flow. can also just not collect email at all and send push notifs only.
    // pub email: Option<String>,

    // TODO(phlip9): other fields? region? language?
}

#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum UserAuthRequest {
    V1(UserAuthRequestV1),
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct UserAuthRequestV1 {
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
    // /// Limit the auth token to a specific Bitcoin network.
    // pub btc_network: Network,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserAuthResponse {
    pub user_auth_token: UserAuthToken,
}

/// An opaque user auth token for authenticating user clients against lexe infra
/// as a particular [`UserPk`](crate::api::UserPk).
///
/// Most user clients should just treat this as an opaque Bearer token with a
/// very short expiration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserAuthToken(pub ByteStr);

/// an [`UserAuthToken`] and its expected expiration time
///
/// * we actually use "true expiration" minus a few seconds so we can re-auth
///   before the token actually expires.
pub struct TokenWithExpiration {
    pub expiration: SystemTime,
    pub token: UserAuthToken,
}

/// A `UserAuthenticator` (1) stores existing fresh auth tokens and (2)
/// authenticates and fetches new auth tokens when they expire.
pub struct UserAuthenticator {
    /// The [`ed25519::KeyPair`] for the [`UserPk`], used to authenticate with
    /// the lexe backend.
    ///
    /// [`UserPk`]: crate::api::UserPk
    user_key_pair: ed25519::KeyPair,

    /// The latest [`UserAuthToken`] with its expected expiration time.
    // NOTE: we intenionally use a tokio `Mutex` here.
    //
    // 1. we want only at-most-one client to try auth'ing at once
    // 2. auth'ing involves IO (send/recv HTTPS request)
    // 3. holding a standard blocking `Mutex` across IO await points is a
    //    Bad Idea^tm, since it'll block all tasks on the runtime (we only use
    //    a single thread for the user node).
    cached_auth_token: tokio::sync::Mutex<Option<TokenWithExpiration>>,
}

// -- impl UserSignupRequest -- //

impl UserSignupRequest {
    pub fn new() -> Self {
        Self::V1(UserSignupRequestV1 {})
    }

    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // for user sign up, the signed signup request is just used to prove
        // ownership of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::VerifyError)
    }
}

impl ed25519::Signable for UserSignupRequest {
    const DOMAIN_SEPARATOR_STR: &'static [u8] =
        b"LEXE-REALM::UserSignupRequest";
}

impl Default for UserSignupRequest {
    fn default() -> Self {
        Self::new()
    }
}

// -- impl UserAuthRequest -- //

impl UserAuthRequest {
    pub fn new(now: SystemTime, token_lifetime_secs: u32) -> Self {
        Self::V1(UserAuthRequestV1 {
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
            .map_err(Error::VerifyError)
    }

    /// Get the `request_timestamp` as a [`SystemTime`]. Returns `Err` if the
    /// `issued_timestamp` is too large to be represented as a unix timestamp
    /// (> 2^63 on linux).
    pub fn request_timestamp(&self) -> Result<SystemTime, Error> {
        let t_secs = match self {
            Self::V1(req) => req.request_timestamp_secs,
        };
        let t_dur_secs = Duration::from_secs(t_secs);
        SystemTime::UNIX_EPOCH
            .checked_add(t_dur_secs)
            .ok_or(Error::InvalidTimestamp)
    }

    /// The requested token lifetime in seconds.
    pub fn lifetime_secs(&self) -> u32 {
        match self {
            Self::V1(req) => req.lifetime_secs,
        }
    }
}

impl ed25519::Signable for UserAuthRequest {
    const DOMAIN_SEPARATOR_STR: &'static [u8] = b"LEXE-REALM::UserAuthRequest";
}

// -- impl UserAuthToken -- //

impl UserAuthToken {
    /// base64 serialize a user auth token from the internal raw bytes.
    pub fn encode_from_raw_bytes(signed_token_bytes: &[u8]) -> Self {
        let b64_token =
            base64::encode_config(signed_token_bytes, base64::URL_SAFE_NO_PAD);
        Self(ByteStr::from(b64_token))
    }

    /// base64 decode the user auth token into the internal raw bytes.
    pub fn decode_into_raw_bytes(&self) -> Result<Vec<u8>, Error> {
        Self::decode_inner_into_raw_bytes(self.0.as_bytes())
    }

    /// base64 decode a string user auth token into the internal raw bytes.
    pub fn decode_inner_into_raw_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
        base64::decode_config(bytes, base64::URL_SAFE_NO_PAD)
            .map_err(|_| Error::Base64Decode)
    }
}

impl fmt::Display for UserAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

// --- impl UserAuthenticator --- //

impl UserAuthenticator {
    /// Create a new `UserAuthenticator` with the auth `api` handle, the
    /// `user_key_pair` (for signing auth requests), and an optional existing
    /// token.
    pub fn new(
        user_key_pair: ed25519::KeyPair,
        maybe_token: Option<TokenWithExpiration>,
    ) -> Self {
        Self {
            user_key_pair,
            cached_auth_token: tokio::sync::Mutex::new(maybe_token),
        }
    }

    /// Try to either (1) return an existing, fresh token or (2) authenticate
    /// with the backend to get a new fresh token (and cache it).
    pub async fn get_token<T: UserAuthApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
    ) -> Result<UserAuthToken, BackendApiError> {
        let mut lock = self.cached_auth_token.lock().await;

        // there's already a fresh token here; just use that.
        if let Some(cached_token) = lock.as_ref() {
            if cached_token.expiration > now {
                return Ok(cached_token.token.clone());
            }
        }

        // no token yet or expired, try to authenticate and get a new token.
        let cached_token = self.authenticate(api, now).await?;
        let token_clone = cached_token.token.clone();
        *lock = Some(cached_token);

        Ok(token_clone)
    }

    /// Create a new [`UserAuthRequest`], sign it, and send the request. Returns
    /// the [`TokenWithExpiration`] if the auth request succeeds.
    ///
    /// NOTE: doesn't update the token cache
    pub async fn authenticate<T: UserAuthApi + ?Sized>(
        &self,
        api: &T,
        now: SystemTime,
    ) -> Result<TokenWithExpiration, BackendApiError> {
        let lifetime = DEFAULT_USER_TOKEN_LIFETIME_SECS;
        let expiration = now + Duration::from_secs(lifetime as u64)
            - Duration::from_secs(15);
        let auth_req = UserAuthRequest::new(now, lifetime);
        let (_, signed_req) = self.user_key_pair.sign_struct(&auth_req)?;
        let resp = api.user_auth(signed_req.cloned()).await?;

        Ok(TokenWithExpiration {
            expiration,
            token: resp.user_auth_token,
        })
    }
}

#[cfg(not(target_env = "sgx"))]
#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip::{
        bcs_roundtrip_proptest, signed_roundtrip_proptest,
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
    fn test_user_auth_request_canonical() {
        bcs_roundtrip_proptest::<UserAuthRequest>();
    }

    #[test]
    fn test_user_auth_request_sign_verify() {
        signed_roundtrip_proptest::<UserAuthRequest>();
    }
}
