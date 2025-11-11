use byte_array::ByteArray;
use common::{
    RefCast,
    serde_helpers::{base64_or_bytes, hexstr_or_bytes},
    time::TimestampMs,
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrPk(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(NostrPk, 32);

/// Upgradeable API struct for a NostrPk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ClientNostrPkStruct {
    pub client_nostr_pk: NostrPk,
}

/// Information about a NWC client.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NwcClient {
    pub client_nostr_pk: NostrPk,
    #[serde(with = "base64_or_bytes")]
    pub ciphertext: Vec<u8>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Query parameters for searching for NWC clients.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNwcClientsParams {
    /// Optionally filter by the client's Nostr PK.
    pub client_nostr_pk: Option<NostrPk>,
}

/// Upserts a NWC client based on the ciphertext encoded by the node and
/// the public key used on Nostr.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateNwcClientRequest {
    pub client_nostr_pk: NostrPk,
    #[serde(with = "base64_or_bytes")]
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct VecNwcClient {
    pub nwc_clients: Vec<NwcClient>,
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn create_new_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateNwcClientRequest>();
    }

    #[test]
    fn nwc_client_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NwcClient>();
    }

    #[test]
    fn nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<ClientNostrPkStruct>();
    }

    #[test]
    fn vec_nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<VecNwcClient>();
    }
}
