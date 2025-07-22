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

use super::user::{NodePkProof, UserPk};
#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    byte_str::ByteStr,
    ed25519::{self, Signed},
};

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
pub enum UserSignupRequestWire {
    V2(UserSignupRequestWireV2),
}

#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserSignupRequestWireV2 {
    pub v1: UserSignupRequestWireV1,

    /// The partner that signed up this user, if any.
    // Added in `node-v0.7.12+`
    pub partner: Option<UserPk>,
}

#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserSignupRequestWireV1 {
    /// The lightning node pubkey in a Proof-of-Key-Possession
    pub node_pk_proof: NodePkProof,

    /// The user's signup code, if provided.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    pub signup_code: Option<String>,
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
/// infra as a particular [`UserPk`].
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

// --- impl UserSignupRequestWire --- //

impl UserSignupRequestWire {
    pub fn node_pk_proof(&self) -> &NodePkProof {
        match self {
            UserSignupRequestWire::V2(v2) => &v2.v1.node_pk_proof,
        }
    }

    pub fn signup_code(&self) -> Option<&str> {
        match self {
            UserSignupRequestWire::V2(v2) => v2.v1.signup_code.as_deref(),
        }
    }

    pub fn partner(&self) -> Option<&UserPk> {
        match self {
            UserSignupRequestWire::V2(v2) => v2.partner.as_ref(),
        }
    }
}

impl ed25519::Signable for UserSignupRequestWire {
    // Name gets cut off to stay within 32 B
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::UserSignupRequestWir");
}

// -- impl UserSignupRequestWireV1 -- //

impl UserSignupRequestWireV1 {
    pub fn deserialize_verify(
        serialized: &[u8],
    ) -> Result<Signed<Self>, Error> {
        // for user sign up, the signed signup request is just used to prove
        // ownership of a user_pk.
        ed25519::verify_signed_struct(ed25519::accept_any_signer, serialized)
            .map_err(Error::UserVerifyError)
    }
}

impl ed25519::Signable for UserSignupRequestWireV1 {
    // Name is different for backwards compat after rename
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::UserSignupRequest");
}

// --- impl UserSignupRequestWireV2 --- //

impl From<UserSignupRequestWireV1> for UserSignupRequestWireV2 {
    fn from(v1: UserSignupRequestWireV1) -> Self {
        Self { v1, partner: None }
    }
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
        Self::decode_slice_into_raw_bytes(self.0.as_bytes())
    }

    /// base64 decode a given string bearer auth token into internal raw bytes.
    pub fn decode_slice_into_raw_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
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

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for BearerAuthToken {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            // Generate a random byte array and encode it
            // This simulates a valid bearer token format
            any::<Vec<u8>>()
                .prop_map(|bytes| {
                    BearerAuthToken::encode_from_raw_bytes(&bytes)
                })
                .boxed()
        }
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
    fn test_user_signup_request_wire_canonical() {
        bcs_roundtrip_proptest::<UserSignupRequestWire>();
    }

    #[test]
    fn test_user_signed_request_wire_sign_verify() {
        signed_roundtrip_proptest::<UserSignupRequestWire>();
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
