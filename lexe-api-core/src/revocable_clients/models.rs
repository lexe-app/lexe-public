//! Request and response types for the revocable client endpoints.

use lexe_common::{
    api::{
        auth::{BearerAuthToken, LexeScope},
        user::UserPk,
    },
    time::TimestampMs,
};
use lexe_crypto::ed25519;
use lexe_serde::{
    base64_or_bytes,
    optopt::{self, none},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use super::RevocableClient;

/// A request to list all revocable clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct ListRevocableClients {
    /// Whether to return only clients which are currently valid.
    pub valid_only: bool,
}

/// A request to create a new revocable client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRevocableClientRequest {
    /// The expiration after which the node should reject this client.
    /// [`None`] indicates that the client will never expire (use carefully!).
    pub expires_at: Option<TimestampMs>,
    /// Optional user-provided label for this client.
    pub label: Option<String>,
    /// The authorization scopes allowed for this client.
    pub scope: LexeScope,
}

/// The response to [`CreateRevocableClientRequest`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRevocableClientResponse {
    /// The user public key associated with these credentials.
    /// Always `Some` since `node-v0.8.11`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_pk: Option<UserPk>,

    /// The client cert pubkey.
    pub pubkey: ed25519::PublicKey,

    /// When this client was created.
    pub created_at: TimestampMs,

    /// The DER-encoded ephemeral issuing CA cert that the client should trust.
    ///
    /// This is just packaged alongside the rest for convenience.
    // NOTE: This client cert goes *last* in the cert chain given to rustls.
    #[serde(with = "base64_or_bytes")]
    pub eph_ca_cert_der: Vec<u8>,

    /// The DER-encoded client cert to present when connecting to the node.
    // NOTE: This client cert goes *first* in the cert chain given to rustls.
    #[serde(with = "base64_or_bytes")]
    pub rev_client_cert_der: Vec<u8>,

    /// The DER-encoded client cert key.
    #[serde(with = "base64_or_bytes")]
    pub rev_client_cert_key_der: Vec<u8>,

    /// A long-lived [`LexeScope::GatewayProxy`] token for connecting to the
    /// user's node via the gateway proxy. Always `Some` for user nodes.
    ///
    /// [`LexeScope::GatewayProxy`]: lexe_common::api::auth::LexeScope::GatewayProxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_proxy_token: Option<BearerAuthToken>,
}

/// A request to update a single [`RevocableClient`].
///
/// All fields except `pubkey` are optional. If a field is `None`, it will not
/// be updated. For example:
///
/// * `expires_at: None` -> don't change
/// * `expires_at: Some(None)` -> set to never expire
/// * `expires_at: Some(TimestampMs(..))` -> set to expire at that time
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq, Arbitrary))]
pub struct UpdateClientRequest {
    /// The pubkey of the client to update.
    pub pubkey: ed25519::PublicKey,

    /// Set this client's expiration (`Some(None)` means never expire).
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub expires_at: Option<Option<TimestampMs>>,

    /// Set this client's label.
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    #[cfg_attr(test, proptest(strategy = "arb::any_label_update()"))]
    pub label: Option<Option<String>>,

    /// Set the authorization scopes allowed for this client.
    #[serde(skip_serializing_if = "none")]
    pub scope: Option<LexeScope>,

    /// Set this to revoke or unrevoke the client. Revocation is permanent, so
    /// you cannot unrevoke a client once it is revoked.
    #[serde(skip_serializing_if = "none")]
    pub is_revoked: Option<bool>,
}

/// The updated [`RevocableClient`] after a successful update.
#[derive(Serialize, Deserialize)]
pub struct UpdateClientResponse {
    pub client: RevocableClient,
}

#[cfg(test)]
mod arb {
    use lexe_common::test_utils::arbitrary;
    use proptest::{option, strategy::Strategy};

    pub fn any_label_update() -> impl Strategy<Value = Option<Option<String>>> {
        option::of(arbitrary::any_option_simple_string())
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn test_update_request_serde() {
        roundtrip::json_string_roundtrip_proptest::<UpdateClientRequest>();
    }

    #[test]
    fn test_list_revocable_clients_serde() {
        roundtrip::query_string_roundtrip_proptest::<ListRevocableClients>();
    }
}
