// user auth v1

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cli::Network;
use crate::ed25519::{self, Signed};

#[derive(Debug, Error)]
pub enum Error {
    #[error("error verifying user signup request: {0}")]
    VerifyError(#[from] ed25519::Error),
}

// TODO(phlip9): do we even need any signup fields?
/// Sign up
#[derive(Deserialize, Serialize)]
pub struct UserSignupRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

/// A user client's request for auth token with certain restrictions.
#[derive(Deserialize, Serialize)]
pub struct UserAuthRequest {
    /// The time the auth token should be issued in UTC Unix time, interpreted
    /// relative to the server clock.
    issued_timestamp: u64,

    /// How long the auth token should be valid, in seconds. At most 1 hour.
    liftime_secs: u32,

    // maybe (?)
    /// Limit the auth token to a specific Bitcoin network.
    btc_network: Network,
}

/// An opaque user auth token for authenticating user clients against lexe infra
/// as a particular [`UserPk`](crate::api::UserPk).
///
/// Most user clients should just treat this as an opaque Bearer token with a
/// very short expiration.
pub struct UserAuthToken(pub String);

// -- impl UserSignupRequest -- //

impl UserSignupRequest {
    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // for user sign up, the signed signup request is just used to prove
        // ownership of a user_pk.
        fn accept_any_signer(_: &ed25519::PublicKey) -> bool {
            true
        }
        ed25519::verify_signed_struct(accept_any_signer, serialized)
            .map_err(Error::VerifyError)
    }
}

impl ed25519::Signable for UserSignupRequest {
    const DOMAIN_SEPARATOR_STR: &'static [u8] =
        b"LEXE-REALM::UserSignupRequest";
}
