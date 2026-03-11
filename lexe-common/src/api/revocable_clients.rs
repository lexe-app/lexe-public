//! Information about a client

use std::collections::HashMap;

use anyhow::anyhow;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{
    api::{auth::Scope, user::UserPk},
    ed25519,
    serde_helpers::{
        base64_or_bytes,
        optopt::{self, none},
    },
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
    pub scope: Scope,
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
}

/// A request to list all revocable clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
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
    pub scope: Option<Scope>,

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

impl RevocableClient {
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
    use crate::test_utils::arbitrary;

    pub fn any_label() -> impl Strategy<Value = Option<String>> {
        static RANGES: &[RangeInclusive<char>] =
            &['0'..='9', 'A'..='Z', 'a'..='z'];
        let any_alphanum_char = proptest::char::ranges(RANGES.into());
        option::of(
            vec(any_alphanum_char, 0..=RevocableClient::MAX_LABEL_LEN)
                .prop_map(String::from_iter),
        )
    }

    #[allow(dead_code)]
    pub fn any_label_update() -> impl Strategy<Value = Option<Option<String>>> {
        option::of(arbitrary::any_option_simple_string())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{root_seed::RootSeed, test_utils::roundtrip};

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

    #[test]
    fn test_update_request_serde() {
        roundtrip::json_string_roundtrip_proptest::<UpdateClientRequest>();
    }

    #[test]
    fn test_get_revocable_clients_serde() {
        roundtrip::query_string_roundtrip_proptest::<GetRevocableClients>();
    }
}
