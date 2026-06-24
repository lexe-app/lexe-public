//! Information about a client

use std::{collections::HashMap, sync::RwLock};

use anyhow::anyhow;
use lexe_common::{
    api::{
        auth::LexeScope,
        revocable_clients::{GetRevocableClientStatus, RevocableClientStatus},
    },
    time::TimestampMs,
};
use lexe_crypto::ed25519;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use self::models::UpdateClientRequest;

/// Request and response types for the revocable client endpoints.
pub mod models;

/// A locked [`RevocableClients`], newtyped so it can implement
/// [`GetRevocableClientStatus`]. Share via `Arc<RevocableClientsHandle>`.
#[derive(Debug)]
pub struct RevocableClientsHandle(pub RwLock<RevocableClients>);

impl GetRevocableClientStatus for RevocableClientsHandle {
    fn get_client_status(
        &self,
        client_pk: &ed25519::PublicKey,
        now: TimestampMs,
    ) -> Option<RevocableClientStatus> {
        let clients = self.0.read().unwrap();
        let client = clients.clients.get(client_pk)?;
        let status = if client.is_revoked {
            RevocableClientStatus::Revoked
        } else if client.is_expired_at(now) {
            RevocableClientStatus::Expired
        } else {
            RevocableClientStatus::Valid
        };
        Some(status)
    }
}

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
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
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
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arb::any_label()")
    )]
    pub label: Option<String>,
    /// The authorization scopes allowed for this client.
    // TODO(max): This scope is currently ineffective.
    pub scope: LexeScope,
    /// Whether this client has been revoked. Revocation is permanent.
    pub is_revoked: bool,
    // TODO(phlip9): add "pausing" a client's access temporarily?
}

impl RevocableClient {
    /// Limit label length to 64 bytes
    pub const MAX_LABEL_LEN: usize = 64;

    /// Whether the client is valid at a given time (not revoked, not expired).
    #[must_use]
    pub fn is_valid_at(&self, now: TimestampMs) -> bool {
        !self.is_revoked && !self.is_expired_at(now)
    }

    /// Whether the client is expired at the given time.
    #[must_use]
    pub fn is_expired_at(&self, now: TimestampMs) -> bool {
        if let Some(expiration) = self.expires_at
            && now > expiration
        {
            return true;
        }

        false
    }

    /// Apply an update to this client, returning a copy with updates applied.
    pub fn update(&self, req: UpdateClientRequest) -> anyhow::Result<Self> {
        let UpdateClientRequest {
            pubkey: req_pubkey,
            expires_at: req_expires_at,
            label: req_label,
            scope: req_scope,
            is_revoked: req_is_revoked,
        } = req;

        let mut out = self.clone();

        if self.pubkey != req_pubkey {
            debug_assert!(false);
            return Err(anyhow!("Cannot update a different client"));
        }

        if let Some(expires_at) = req_expires_at {
            // TODO(max): Maybe need some validation here
            out.expires_at = expires_at;
        }

        if let Some(maybe_label) = req_label {
            if let Some(label) = &maybe_label
                && label.len() > Self::MAX_LABEL_LEN
            {
                return Err(anyhow!(
                    "Label must not be longer than {} bytes",
                    Self::MAX_LABEL_LEN,
                ));
            }
            out.label = maybe_label;
        }

        if let Some(scope) = req_scope {
            // TODO(max): Need some validation here; can't request broader
            // scope, only some clients can call, etc.
            out.scope = scope;
        }

        if let Some(revoke) = req_is_revoked {
            if self.is_revoked && !revoke {
                return Err(anyhow!("Cannot unrevoke a client"));
            }
            out.is_revoked = revoke;
        }

        Ok(out)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arb {
    use std::ops::RangeInclusive;

    use proptest::{collection::vec, option, strategy::Strategy};

    use super::*;

    pub fn any_label() -> impl Strategy<Value = Option<String>> {
        static RANGES: &[RangeInclusive<char>] =
            &['0'..='9', 'A'..='Z', 'a'..='z'];
        let any_alphanum_char = proptest::char::ranges(RANGES.into());
        option::of(
            vec(any_alphanum_char, 0..=RevocableClient::MAX_LABEL_LEN)
                .prop_map(String::from_iter),
        )
    }
}

#[cfg(test)]
mod test {
    use lexe_common::root_seed::RootSeed;

    use super::*;

    #[test]
    fn rev_client_ser_basic() {
        let client1 = RevocableClient {
            pubkey: *RootSeed::from_u64(1).derive_user_key_pair().public_key(),
            created_at: TimestampMs::from_secs_u32(69),
            expires_at: Some(TimestampMs::from_secs_u32(420)),
            label: Some("deez".to_string()),
            scope: LexeScope::All,
            is_revoked: false,
        };
        let client_json = serde_json::to_string_pretty(&client1).unwrap();
        // println!("{client_json}");
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
