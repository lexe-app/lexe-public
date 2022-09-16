// user auth v1

#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cli::Network;
use crate::ed25519::{self, Signed};

#[derive(Debug, Error)]
pub enum Error {
    #[error("error verifying user signup request: {0}")]
    VerifyError(#[from] ed25519::Error),
}

#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum UserSignupRequest {
    V1(UserSignupRequestV1),
}

// TODO(phlip9): do we even need any signup fields?
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct UserSignupRequestV1 {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum UserAuthRequest {
    V1(UserAuthRequestV1),
}

/// A user client's request for auth token with certain restrictions.
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct UserAuthRequestV1 {
    /// The time the auth token should be issued in UTC Unix time, interpreted
    /// relative to the server clock.
    issued_timestamp: u64,

    /// How long the auth token should be valid, in seconds. At most 1 hour.
    liftime_secs: u32,

    // maybe (?)
    /// Limit the auth token to a specific Bitcoin network.
    btc_network: Network,
}

#[derive(Deserialize, Serialize)]
pub struct UserAuthResponse {
    pub user_auth_token: UserAuthToken,
}

/// An opaque user auth token for authenticating user clients against lexe infra
/// as a particular [`UserPk`](crate::api::UserPk).
///
/// Most user clients should just treat this as an opaque Bearer token with a
/// very short expiration.
#[derive(Deserialize, Serialize)]
pub struct UserAuthToken(pub String);

// -- impl UserSignupRequest -- //

impl UserSignupRequest {
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
}

impl ed25519::Signable for UserAuthRequest {
    const DOMAIN_SEPARATOR_STR: &'static [u8] = b"LEXE-REALM::UserAuthRequest";
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
