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

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrPk(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(NostrPk, 32);

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, RefCast)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[repr(transparent)]
pub struct NostrEventId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(NostrEventId, 32);

/// Upgradeable API struct for a NostrPk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NostrPkStruct {
    pub nostr_pk: NostrPk,
}

/// Wallet service information stored in the DB.
///
/// Ciphertext is encrypted using node's master key and stores the
/// wallet service secret key and client public key to be used on nip47
/// communication protocol.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct DbNwcWallet {
    pub wallet_nostr_pk: NostrPk,
    #[serde(with = "base64_or_bytes")]
    pub ciphertext: Vec<u8>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// Information about an existing NWC wallet.
///
/// This is used for listing wallets to the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NwcWalletInfo {
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

/// Response to list NWC wallet.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ListNwcWalletResponse {
    pub wallets: Vec<NwcWalletInfo>,
}

/// Query parameters to search for NWC wallets.
///
/// This params adds optinal filtering besides the user_pk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNwcWalletsParams {
    /// Optionally filter by the wallet's Nostr PK.
    pub wallet_nostr_pk: Option<NostrPk>,
}

/// Upserts a NWC wallet in the database based on the ciphertext encoded by
/// the node and the public key used on Nostr.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateDbNwcWalletRequest {
    pub wallet_nostr_pk: NostrPk,
    #[serde(with = "base64_or_bytes")]
    pub ciphertext: Vec<u8>,
}

/// Request to create a new NWC wallet.
///
/// Keys are generated on the Node and stored safely encrypted in the DB.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct CreateNwcWalletRequest {
    /// Human-readable label for this client.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub label: String,
}

/// Request to update an existing NWC wallet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateNwcWalletRequest {
    /// The wallet service public key identifying the wallet to update.
    pub wallet_nostr_pk: NostrPk,
    /// Updated human-readable label for this client.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub label: String,
}

/// Response for creating a new NWC wallet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct CreateNwcWalletResponse {
    /// The wallet service public key for this wallet.
    pub wallet_nostr_pk: NostrPk,
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

/// Response for updating an existing NWC wallet.
///
/// NOTE: this response does not contain the connection string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateNwcWalletResponse {
    /// Information about the updated NWC wallet.
    pub wallet_info: NwcWalletInfo,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct VecNwcWallet {
    pub nwc_wallets: Vec<DbNwcWallet>,
}

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
    fn update_db_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateDbNwcWalletRequest>();
    }

    #[test]
    fn create_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<CreateNwcWalletRequest>();
    }

    #[test]
    fn update_nwc_client_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateNwcWalletRequest>();
    }

    #[test]
    fn create_nwc_client_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<CreateNwcWalletResponse>();
    }

    #[test]
    fn update_nwc_client_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateNwcWalletResponse>();
    }

    #[test]
    fn nwc_client_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<DbNwcWallet>();
    }

    #[test]
    fn nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NostrPkStruct>();
    }

    #[test]
    fn vec_nostr_client_pk_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<VecNwcWallet>();
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
}
