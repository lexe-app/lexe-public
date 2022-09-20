// user auth v1

use std::time::{Duration, SystemTime};

#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ed25519::{self, Signed};

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
    pub liftime_secs: u32,
    // /// Limit the auth token to a specific Bitcoin network.
    // pub btc_network: Network,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UserAuthResponse {
    pub user_auth_token: OpaqueUserAuthToken,
}

/// An opaque user auth token for authenticating user clients against lexe infra
/// as a particular [`UserPk`](crate::api::UserPk).
///
/// Most user clients should just treat this as an opaque Bearer token with a
/// very short expiration.
#[derive(Debug, Deserialize, Serialize)]
pub struct OpaqueUserAuthToken(pub String);

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
    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // likewise, user/node auth is (currently) just used to prove ownership
        // of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::VerifyError)
    }

    /// Get the `request_timestamp` as a [`SystemTime`]. Returns `None` if the
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
            Self::V1(req) => req.liftime_secs,
        }
    }
}

impl ed25519::Signable for UserAuthRequest {
    const DOMAIN_SEPARATOR_STR: &'static [u8] = b"LEXE-REALM::UserAuthRequest";
}

// -- impl OpaqueUserAuthToken -- //

impl OpaqueUserAuthToken {
    /// base64 deserialize the user auth token
    pub fn from_bytes(signed_token_bytes: &[u8]) -> Self {
        Self(base64::encode_config(
            signed_token_bytes,
            base64::URL_SAFE_NO_PAD,
        ))
    }

    /// base64 serialize the user auth token
    pub fn into_bytes(&self) -> Result<Vec<u8>, Error> {
        base64::decode_config(self.0.as_str(), base64::URL_SAFE_NO_PAD)
            .map_err(|_| Error::Base64Decode)
    }
}

#[cfg(not(target_env = "sgx"))]
#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::{assert_bcs_roundtrip, assert_signed_roundtrip};

    #[test]
    fn test_user_signup_request_canonical() {
        assert_bcs_roundtrip::<UserSignupRequest>();
    }

    #[test]
    fn test_user_signed_request_sign_verify() {
        assert_signed_roundtrip::<UserSignupRequest>();
    }

    #[test]
    fn test_user_auth_request_canonical() {
        assert_bcs_roundtrip::<UserAuthRequest>();
    }

    #[test]
    fn test_user_auth_request_sign_verify() {
        assert_signed_roundtrip::<UserAuthRequest>();
    }
}
