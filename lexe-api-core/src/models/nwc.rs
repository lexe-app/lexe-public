use byte_array::ByteArray;
#[cfg(any(test, feature = "test-utils"))]
use common::test_utils::arbitrary;
use common::{
    RefCast,
    serde_helpers::{base64_or_bytes, hexstr_or_bytes},
    time::TimestampMs,
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrPk(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(NostrPk, 32);
byte_array::impl_debug_display_as_hex!(NostrPk);

#[derive(Copy, Clone, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrEventId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(NostrEventId, 32);
byte_array::impl_debug_display_as_hex!(NostrEventId);

/// A 32-byte Nostr secret key.
#[derive(Copy, Clone, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrSk(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

byte_array::impl_byte_array!(NostrSk, 32);
byte_array::impl_debug_display_redacted!(NostrSk);

/// Upgradeable API struct for a NostrPk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NostrPkStruct {
    pub nostr_pk: NostrPk,
}

/// A NWC client as represented in the DB, minus the timestamp fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct DbNwcClientFields {
    /// The NWC client app's Nostr public key (identifies the caller).
    pub client_nostr_pk: NostrPk,
    /// The wallet service's Nostr public key (identifies this wallet).
    pub wallet_nostr_pk: NostrPk,
    /// VFS-encrypted client secret data (wallet SK + label).
    #[serde(with = "base64_or_bytes")]
    pub ciphertext: Vec<u8>,
}

/// Full NWC client record from the DB.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct DbNwcClient {
    #[serde(flatten)]
    pub fields: DbNwcClientFields,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Information about an existing NWC client.
///
/// This is used for listing clients to the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NwcClientInfo {
    /// The client public key (identifies the caller of this connection).
    pub client_nostr_pk: NostrPk,
    /// The wallet service public key (identifies this connection).
    pub wallet_nostr_pk: NostrPk,
    /// Human-readable label for this connection.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub label: String,
    /// When this connection was created.
    pub created_at: TimestampMs,
    /// When this connection was last updated.
    pub updated_at: TimestampMs,
}

// ---- Requests and responses App <-> Backend ---- //

/// Response to list NWC clients.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ListNwcClientResponse {
    pub clients: Vec<NwcClientInfo>,
}

/// Request to create a new NWC client.
///
/// Keys are generated on the Node and stored safely encrypted in the DB.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct CreateNwcClientRequest {
    /// Human-readable label for this client.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub label: String,
}

/// Request to update an existing NWC client.
// TODO(maurice): Add option to update budget limits, budget restriction type
// (single-use, monthly, yearly, total, etc.).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateNwcClientRequest {
    /// The client public key identifying the client to update.
    pub client_nostr_pk: NostrPk,
    /// Updated human-readable label for this client.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    pub label: Option<String>,
}

/// Response for creating a new NWC client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct CreateNwcClientResponse {
    /// The wallet service public key for this wallet.
    pub wallet_nostr_pk: NostrPk,
    /// The client public key for this client.
    pub client_nostr_pk: NostrPk,
    /// Human-readable label for this client.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub label: String,
    /// The NWC connection string (nostr+walletconnect://..).
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub connection_string: String,
}

/// Response for updating an existing NWC client.
///
/// NOTE: this response does not contain the connection string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateNwcClientResponse {
    /// Information about the updated NWC client.
    pub client_info: NwcClientInfo,
}

// ---- Requests and responses Node <-> Backend ---- //

/// Query parameters to search for NWC clients.
///
/// This params adds optinal filtering besides the user_pk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNwcClients {
    /// Optionally filter by the client's Nostr PK.
    pub client_nostr_pk: Option<NostrPk>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct VecDbNwcClient {
    pub nwc_clients: Vec<DbNwcClient>,
}

// ---- Requests and responses  Nostr-bridge <-> Node ---- //

/// Request from nostr-bridge to user node with an encrypted NWC request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NwcRequest {
    /// The nostr PK of the sender of the message (also, the NWC client app).
    pub client_nostr_pk: NostrPk,
    /// The Nostr PK of the recipient (the wallet service PK).
    pub wallet_nostr_pk: NostrPk,
    /// The nostr event hex id. Used to build the response nostr event.
    pub event_id: NostrEventId,
    /// The NIP-44 v2 encrypted payload containing the NWC request.
    #[serde(with = "base64_or_bytes")]
    pub nip44_payload: Vec<u8>,
}

/// Generic signed nostr event.
///
/// Used for to forward nostr events from the node to nostr-bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NostrSignedEvent {
    /// Base64 encoded string of the Json-encoded event.
    #[serde(with = "base64_or_bytes")]
    pub event: Vec<u8>,
}

/// NIP-47 protocol structures.
pub mod nip47 {
    use std::fmt;

    use serde::{Deserialize, Serialize};

    /// NWC request method.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum NwcMethod {
        GetInfo,
        MakeInvoice,
        LookupInvoice,
        ListTransactions,
        GetBalance,
        MultiPayKeysend,
        PayKeysend,
        MultiPayInvoice,
        PayInvoice,
    }

    /// Parameters for `make_invoice` command.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct MakeInvoiceParams {
        /// Amount in millisats.
        #[serde(rename = "amount")]
        pub amount_msat: u64,
        /// Invoice description.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        /// Invoice description hash (32 bytes hex).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description_hash: Option<String>,
        /// Invoice expiry in seconds.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub expiry: Option<u32>,
        /// Generic metadata (e.g., zap/boostagram details). Optional and
        /// ignored.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub metadata: Option<serde_json::Value>,
    }

    /// NWC request payload (decrypted).
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct NwcRequestPayload {
        pub method: NwcMethod,
        pub params: serde_json::Value,
    }

    /// Result for `get_info` command.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct GetInfoResult {
        pub alias: String,
        pub color: String,
        pub pubkey: String,
        pub network: String,
        pub block_height: u32,
        pub block_hash: String,
        pub methods: Vec<String>,
    }

    /// Result for `make_invoice` command.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct MakeInvoiceResult {
        /// BOLT11 invoice string.
        pub invoice: String,
        /// Payment hash (hex).
        pub payment_hash: String,
    }

    /// NWC error codes.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    pub enum NwcErrorCode {
        RateLimited,
        NotImplemented,
        InsufficientBalance,
        QuotaExceeded,
        Restricted,
        Unauthorized,
        Internal,
        Other,
    }

    /// NWC error response.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct NwcError {
        pub code: NwcErrorCode,
        pub message: String,
    }

    impl NwcError {
        pub fn new(code: NwcErrorCode, message: String) -> Self {
            Self { code, message }
        }

        pub fn not_implemented(message: impl fmt::Display) -> Self {
            let message = format!("Not implemented: {message:#}");
            Self {
                code: NwcErrorCode::NotImplemented,
                message,
            }
        }

        pub fn other(message: impl fmt::Display) -> Self {
            let message = format!("Other error: {message:#}");
            Self {
                code: NwcErrorCode::Other,
                message,
            }
        }

        pub fn internal(message: impl fmt::Display) -> Self {
            let message = format!("Internal error: {message:#}");
            Self {
                code: NwcErrorCode::Internal,
                message,
            }
        }
    }

    /// NWC response payload (to be encrypted).
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct NwcResponsePayload {
        pub result_type: NwcMethod,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub error: Option<NwcError>,
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn db_nwc_client_fields_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<DbNwcClientFields>();
    }

    #[test]
    fn create_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<CreateNwcClientRequest>();
    }

    #[test]
    fn update_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateNwcClientRequest>();
    }

    #[test]
    fn create_nwc_client_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<CreateNwcClientResponse>();
    }

    #[test]
    fn update_nwc_client_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateNwcClientResponse>();
    }

    #[test]
    fn nwc_client_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<DbNwcClient>();
    }

    #[test]
    fn nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NostrPkStruct>();
    }

    #[test]
    fn vec_nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<VecDbNwcClient>();
    }

    #[test]
    fn nwc_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NwcRequest>();
    }

    #[test]
    fn nostr_signed_event_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NostrSignedEvent>();
    }

    #[test]
    fn nostr_event_id_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NostrEventId>();
    }

    #[test]
    fn nostr_sk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NostrSk>();
    }
}
