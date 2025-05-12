//! Information about a client

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    api::auth::Scope, ed25519, serde_helpers::base64_or_bytes,
    time::TimestampMs,
};

/// All revocable clients which have ever been created.
///
/// This struct must be persisted in a rollback-resistant data store.
// We don't *really* need to persist revoked clients but might as well keep them
// around for historical reference. We can prune them later if needed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RevocableClients {
    pub clients: HashMap<ed25519::PublicKey, RevocableClient>,
}

impl RevocableClients {
    /// A user shouldn't need more than 100 clients.
    pub const MAX_LEN: usize = 100;

    /// An iterator over all clients which are valid right now.
    pub fn iter_valid(
        &self,
    ) -> impl Iterator<Item = (&ed25519::PublicKey, &RevocableClient)> {
        self.iter_valid_at(TimestampMs::now())
    }

    /// An iterator over all clients which are valid at the given time.
    pub fn iter_valid_at(
        &self,
        now: TimestampMs,
    ) -> impl Iterator<Item = (&ed25519::PublicKey, &RevocableClient)> {
        self.clients
            .iter()
            .filter(|(_k, v)| !v.is_revoked)
            .filter(move |(_k, v)| !v.is_expired_at(now))
    }
}

/// Information about a revocable client.
/// Each client is issued a `RevocableClientCert` whose pubkey is saved here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevocableClient {
    /// The client's cert pubkey.
    // TODO(max): In the future, bearer auth tokens could be issued to this pk.
    pub pubkey: ed25519::PublicKey,
    /// When we first issued the client cert and created this client.
    pub created_at: TimestampMs,
    /// The time after which the server will no longer accept this client.
    /// [`None`] indicates that the client will never expire (use carefully!).
    /// This expiration time can be extended at any time.
    pub expires_at: Option<TimestampMs>,
    /// Optional user-provided label for this client.
    pub label: Option<String>,
    /// The authorization scopes allowed for this client.
    // TODO(max): This scope is currently ineffective.
    pub scope: Scope,
    /// Whether this client has been revoked. Revocation should be permanent,
    /// so this metadata can be pruned if needed.
    pub is_revoked: bool,
}

impl RevocableClient {
    /// Limit label length to 64 bytes
    pub const MAX_LABEL_LEN: usize = 64;

    /// Whether the client is expired right now.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(TimestampMs::now())
    }

    /// Whether the client is expired at the given time.
    #[must_use]
    pub fn is_expired_at(&self, now: TimestampMs) -> bool {
        if let Some(expiration) = self.expires_at {
            if now > expiration {
                return true;
            }
        }

        false
    }
}

/// A request to list all revocable clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetRevocableClients {
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
    pub scope: Scope,
}

/// The response to [`CreateRevocableClientRequest`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRevocableClientResponse {
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
}

/// A request to update a revocable client's expiration time to the given time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateClientExpiration {
    pub pubkey: ed25519::PublicKey,
    /// The time after which the server should reject this client.
    /// Setting this to [`None`] removes the expiration (use carefully!).
    pub expires_at: Option<TimestampMs>,
}

/// A request to update a revocable client's label to the given label.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateClientLabel {
    pub pubkey: ed25519::PublicKey,
    /// The label to use for this client.
    pub label: Option<String>,
}

/// A request to update a revocable client's scope to the given scope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateClientScope {
    pub pubkey: ed25519::PublicKey,
    /// The new authorization scopes to be allowed for this client.
    pub scope: Scope,
}

/// A request to revoke a revocable client.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevokeClient {
    pub pubkey: ed25519::PublicKey,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::root_seed::RootSeed;

    #[test]
    fn rev_client_ser_basic() {
        let client1 = RevocableClient {
            pubkey: *RootSeed::from_u64(1).derive_user_key_pair().public_key(),
            created_at: TimestampMs::from_secs_u32(69),
            expires_at: Some(TimestampMs::from_secs_u32(420)),
            label: Some("deez".to_string()),
            scope: Scope::All,
            is_revoked: false,
        };
        let client_json = serde_json::to_string_pretty(&client1).unwrap();
        println!("{client_json}");
        let client_json_snapshot = r#"{
  "pubkey": "aa8e3e1a9bffdb073507f23474100619fdd4e392ef0ff1e89348252f287a06fc",
  "created_at": 69000,
  "expires_at": 420000,
  "label": "deez",
  "scope": "All",
  "is_revoked": false
}"#;
        assert_eq!(client_json, client_json_snapshot);

        let client2 =
            serde_json::from_str::<RevocableClient>(&client_json).unwrap();
        assert_eq!(client1, client2);
    }
}
