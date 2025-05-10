//! Information about a client

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{api::auth::Scope, ed25519, time::TimestampMs};

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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevocableClient {
    /// The client's cert pubkey.
    // TODO(max): In the future, bearer auth tokens could be issued to this pk.
    pub pubkey: ed25519::PublicKey,
    /// When we first issued the client cert and created this client.
    pub created_at: TimestampMs,
    /// The time after which the server will no longer accept this client.
    /// [`None`] indicates that the client will never expire (use carefully!).
    /// This expiration time can be extended at any time.
    pub expiration: Option<TimestampMs>,
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
    /// Whether the client is expired right now.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(TimestampMs::now())
    }

    /// Whether the client is expired at the given time.
    #[must_use]
    pub fn is_expired_at(&self, now: TimestampMs) -> bool {
        if let Some(expiration) = self.expiration {
            if now > expiration {
                return true;
            }
        }

        false
    }
}
