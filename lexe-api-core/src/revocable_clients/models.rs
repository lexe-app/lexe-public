//! Request and response types for the revocable client endpoints.

use std::{borrow::Cow, collections::HashMap};

use lexe_common::{
    api::{auth::BearerAuthToken, user::UserPk},
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

use super::{
    RevocableClient, grandfathered_permissions, scopes::ClientPermissions,
};
use crate::budget::models::{BudgetInfo, BudgetParams, UpdateBudgetParams};

/// The response to a `client_info` request: the caller's own authentication
/// kind and granted authorization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GetClientInfoResponse {
    /// Root seed or client credentials.
    pub kind: CredentialKind,

    /// The client cert pubkey. `Some` iff `kind` is `ClientCredentials`.
    pub pubkey: Option<ed25519::PublicKey>,

    /// When the client was created. `Some` iff `kind` is `ClientCredentials`.
    pub created_at: Option<TimestampMs>,

    /// When the client expires. [`None`] means it never expires.
    /// Root seed clients never expire.
    pub expires_at: Option<TimestampMs>,

    /// The client's label, if any.
    pub label: Option<String>,

    /// The caller's granted authorization.
    /// Root seed clients hold the equivalent of the `full` scope.
    #[serde(deserialize_with = "ClientPermissions::deserialize_drop_unknown")]
    pub permissions: ClientPermissions,

    /// Every permission the caller currently holds.
    //
    // Since clients aren't able to compute the scope -> permissions mapping,
    // we compute this for them server-side.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_permissions: Vec<Cow<'static, str>>,

    /// The caller's budget. [`None`] means no spending limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetInfo>,
}

/// How a client authenticated: root seed or client credentials.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    RootSeed,
    ClientCredentials,
}

/// A request to list all revocable clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct ListRevocableClients {
    /// Whether to return only clients which are currently valid.
    pub valid_only: bool,
}

/// The response to [`ListRevocableClients`].
#[derive(Serialize, Deserialize)]
pub struct ListRevocableClientsResponse {
    pub clients: HashMap<ed25519::PublicKey, RevocableClientWithBudget>,
}

/// A request to create a new revocable client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRevocableClientRequest {
    /// The expiration after which the node should reject this client.
    /// [`None`] indicates that the client will never expire (use carefully!).
    pub expires_at: Option<TimestampMs>,
    /// Optional user-provided label for this client.
    pub label: Option<String>,
    /// The spending budget to enforce for this client.
    //
    // compat: Added in node-v0.10.9.
    #[serde(default)]
    pub budget: BudgetParams,
    /// The authorization to grant this client.
    //
    // NOTE: This is safe to `default` to `full` because an omission can only
    // come from a trusted app or SDK client predating `permissions`, which
    // created clients with de-facto `full` access. An adversary cannot use this
    // codepath to escalate, because the server enforces scope attenuation,
    // such that only a `full` caller can trigger the default branch.
    //
    // compat: Remove this `default` once all clients are node-v0.9.12+.
    #[serde(default = "grandfathered_permissions")]
    pub permissions: ClientPermissions,
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

    /// Every permission the created client currently holds.
    //
    // Since clients aren't able to compute the scope -> permissions mapping,
    // we compute this for them server-side so that clients have access to the
    // effective permissions immediately upon creation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_permissions: Vec<Cow<'static, str>>,

    /// The created client's budget. [`None`] means no spending limit.
    pub budget: Option<BudgetInfo>,

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

    /// Set the authorization granted to this client.
    #[serde(skip_serializing_if = "none")]
    pub permissions: Option<ClientPermissions>,

    /// Set this to revoke or unrevoke the client. Revocation is permanent, so
    /// you cannot unrevoke a client once it is revoked.
    #[serde(skip_serializing_if = "none")]
    pub is_revoked: Option<bool>,
}

/// A request to update a single [`RevocableClient`] and its budget.
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq, Arbitrary))]
pub struct UpdateClientWithBudgetRequest {
    // Flattening keeps this readable from older apps (<node-v0.10.9), which
    // send an `UpdateClientRequest`.
    #[serde(flatten)]
    pub client: UpdateClientRequest,
    /// Update the client's spending budget.
    // compat: Added in node-v0.10.9.
    #[serde(default)]
    pub budget: UpdateBudgetParams,
}

/// The updated [`RevocableClient`] after a successful update.
#[derive(Serialize, Deserialize)]
pub struct UpdateClientResponse {
    pub client: RevocableClientWithBudget,
}

impl UpdateClientWithBudgetRequest {
    /// Whether this update changes the client's grant: its expiration,
    /// permissions, or budget. Relabeling and revoking don't.
    pub fn changes_grant(&self) -> bool {
        self.client.expires_at.is_some()
            || self.client.permissions.is_some()
            || !self.budget.is_noop()
    }
}

impl From<UpdateClientRequest> for UpdateClientWithBudgetRequest {
    fn from(client: UpdateClientRequest) -> Self {
        Self {
            client,
            budget: UpdateBudgetParams::default(),
        }
    }
}

/// A [`RevocableClient`] with its [`BudgetInfo`].
//
// A budget describes its client, so responses return them together. However,
// clients and budgets are stored separately, so the budget can't be a field of
// `RevocableClient`. Instead, this struct pairs them.
#[derive(Serialize, Deserialize)]
pub struct RevocableClientWithBudget {
    // Flattening keeps this readable by older apps (<node-v0.10.9), which
    // expect a `RevocableClient`.
    #[serde(flatten)]
    pub client: RevocableClient,
    /// The client's budget. [`None`] means no spending limit.
    pub budget: Option<BudgetInfo>,
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
    use std::collections::HashMap;

    use lexe_common::test_utils::{arbitrary, roundtrip};
    use lexe_crypto::rng::FastRng;
    use proptest::prelude::any;

    use super::*;
    use crate::revocable_clients::{RevocableClients, scopes::Scope};

    // --- Ser/de roundtrips --- //

    #[test]
    fn test_list_revocable_clients_serde() {
        roundtrip::query_string_roundtrip_proptest::<ListRevocableClients>();
    }

    #[test]
    fn test_update_request_serde() {
        roundtrip::json_string_roundtrip_proptest::<UpdateClientRequest>();
        roundtrip::json_string_roundtrip_proptest::<
            UpdateClientWithBudgetRequest,
        >();
    }

    // --- Compat --- //

    /// Ensure that `RevocableClient`, read by old clients, can be deserialized
    /// from `RevocableClientWithBudget`, sent by new nodes.
    //
    //   Old client            New node
    //   `RevocableClient` <-- `RevocableClientWithBudget`
    //
    // In node-v0.10.9, responses replaced `RevocableClient` with
    // `RevocableClientWithBudget` to add budgets, e.g. in
    // `UpdateClientResponse::client`.
    #[test]
    fn old_client_reads_revocable_client_with_budget() {
        let mut rng = FastRng::from_u64(202607211148);
        let generated = arbitrary::gen_values(
            &mut rng,
            (any::<RevocableClient>(), any::<Option<BudgetInfo>>()),
            8,
        );

        for (client, budget) in generated {
            let new_resp_struct = RevocableClientWithBudget { client, budget };

            // Serialize `RevocableClientWithBudget`, sent by new nodes, to JSON
            let new_resp_json =
                serde_json::to_string(&new_resp_struct).unwrap();

            // Deserialize JSON into `RevocableClient`, read by old clients
            let old_resp_struct: RevocableClient =
                serde_json::from_str(&new_resp_json).unwrap();

            assert_eq!(old_resp_struct, new_resp_struct.client);
        }
    }

    /// Ensure that `RevocableClients`, read by old clients, can be deserialized
    /// from `ListRevocableClientsResponse`, sent by new nodes.
    //
    //   Old client             New node
    //   `RevocableClients` <-- `ListRevocableClientsResponse`
    //
    // Each response is a `HashMap` of client entries: `RevocableClient`s in
    // `RevocableClients`, and `RevocableClientWithBudget`s in
    // `ListRevocableClientsResponse`. The entries on their own are already
    // tested by `old_client_reads_revocable_client_with_budget`. However, the
    // entries stay compatible through `#[serde(flatten)]`, which can behave
    // differently inside a `HashMap`, so they need testing as `HashMap` values
    // too.
    #[test]
    fn old_client_reads_list_revocable_clients_response() {
        let mut rng = FastRng::from_u64(202607201611);
        let generated = arbitrary::gen_values(
            &mut rng,
            (any::<RevocableClient>(), any::<Option<BudgetInfo>>()),
            8,
        );
        let clients = generated
            .into_iter()
            .map(|(client, budget)| {
                (client.pubkey, RevocableClientWithBudget { client, budget })
            })
            .collect::<HashMap<_, _>>();
        let new_resp_struct = ListRevocableClientsResponse { clients };

        // Serialize `ListRevocableClientsResponse`, sent by new nodes, to JSON
        let new_resp_json = serde_json::to_string(&new_resp_struct).unwrap();

        // Deserialize JSON into `RevocableClients`, read by old clients
        let old_resp_struct: RevocableClients =
            serde_json::from_str(&new_resp_json).unwrap();

        // Every client survives unchanged, and none are dropped
        assert_eq!(
            old_resp_struct.clients.len(),
            new_resp_struct.clients.len()
        );
        for (pk, old_resp_client) in &old_resp_struct.clients {
            let new_resp_client = &new_resp_struct.clients[pk].client;
            assert_eq!(old_resp_client, new_resp_client);
        }
    }

    /// Create requests from old clients predate the `permissions` key and
    /// instead carry a legacy `scope` field. They must still deserialize
    /// (`scope` ignored), grandfathered to `full`.
    #[test]
    fn create_client_request_backwards_compat() {
        let json =
            r#"{"expires_at": null, "label": "old client", "scope": "All"}"#;
        let req =
            serde_json::from_str::<CreateRevocableClientRequest>(json).unwrap();
        assert_eq!(
            req.permissions,
            ClientPermissions::from_single_scope(Scope::Full)
        );
    }
}
