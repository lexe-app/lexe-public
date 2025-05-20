//! # Lexe SDK API request and response types
//!
//! ## Guidelines
//!
//! - **Simple**: Straightforward consumption by newbie developers via a JSON
//!   REST API (Lexe Sidecar SDK) or via language bindings (Lexe SDK).
//!
//!   - *Minimal nesting* means users don't have to define multiple structs per
//!     request / response.
//!   - *Fewer fields* means fewer long-term compatibility commitments.
//!
//! - **User-facing docs**: The doc strings here will be used to generate Lexe's
//!   API references and thus should be written for SDK users, not Lexe
//!   developers. Internally facing docs can use regular comments.
//!
//! - **Document serialization and units**: When newtypes are used, be sure to
//!   document how users should interpret the serialized form, as they do not
//!   have access to the newtype information. For example:
//!
//!   - [`UserPk`]s and [`NodePk`]s are serialized as hex; mention it.
//!   - [`Amount`]s are serialized as sats; mention it.
//!   - [`TimestampMs`] is serialized as *milliseconds* since the UNIX epoch
//!     instead of seconds, which many users expect; mention it.
//!   - [`semver::Version`]s don't use a `v-` prefix; give an example: `0.6.9`.
//!
//! - **Serialize `null`**: Don't use `#[serde(skip_serializing_if =
//!   "Option::is_none")]` as serializing `null` fields in the responses makes
//!   it clear to SDK users that information could returned there in future
//!   responses.
//!
//! [`UserPk`]: common::api::user::UserPk
//! [`NodePk`]: common::api::user::NodePk
//! [`Amount`]: common::ln::amount::Amount
//! [`TimestampMs`]: common::time::TimestampMs

use common::{
    api::user::{NodePk, UserPk},
    enclave,
    ln::{
        amount::Amount,
        invoice::LxInvoice,
        payments::{LxPaymentHash, LxPaymentSecret, PaymentIndex},
    },
    time::TimestampMs,
};
use lexe_api_core::models::command;
use serde::{Deserialize, Serialize};

use crate::types::SdkPayment;

/// Information about a Lexe node.
// Simple version of `lexe_api::command::NodeInfo`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SdkNodeInfoResponse {
    /// The node's current semver version, e.g. `0.6.9`.
    pub version: semver::Version,
    /// The hex-encoded SGX 'measurement' of the current node.
    /// The measurement is the hash of the enclave binary.
    pub measurement: enclave::Measurement,
    /// The hex-encoded ed25519 user public key used to identify a Lexe user.
    /// The user keypair is derived from the root seed.
    pub user_pk: UserPk,
    /// The hex-encoded secp256k1 Lightning node public key; the `node_id`.
    pub node_pk: NodePk,

    /// The sum of our `lightning_balance` and our `onchain_balance`, in sats.
    pub balance: Amount,

    /// Total Lightning balance in sats, summed over all of our channels.
    pub lightning_balance: Amount,
    /// Total usable Lightning balance in sats, summing all *usable* channels.
    pub usable_lightning_balance: Amount,
    /// The maximum amount that we could possibly send over Lightning, in sats.
    ///
    /// - Is strictly less than `usable_lightning_balance`.
    /// - Accounts for usable channels, LSP fees, and other LN protocol limits
    ///   including channel reserves, pending HTLCs, per-HTLC limits, etc.
    /// - Exactly this amount may be sendable only in very specific scenarios,
    ///   such as paying another Lexe user.
    pub max_sendable_lightning_balance: Amount,

    /// Total on-chain balance in sats, including unconfirmed funds.
    // `OnchainBalance::total`
    pub onchain_balance: Amount,
    /// Trusted on-chain balance in sats, including only confirmed funds and
    /// unconfirmed outputs originating from our own wallet.
    // Equivalent to BDK's `trusted_spendable`, but with a better name.
    pub trusted_onchain_balance: Amount,

    /// The total number of Lightning channels.
    pub num_channels: usize,
    /// The number of channels which are currently usable, i.e. `channel_ready`
    /// messages have been exchanged and the channel peer is online.
    /// Is always less than or equal to `num_channels`.
    pub num_usable_channels: usize,
}

impl From<lexe_api_core::models::command::NodeInfo> for SdkNodeInfoResponse {
    fn from(info: lexe_api_core::models::command::NodeInfo) -> Self {
        let lightning_balance = info.lightning_balance.total();
        let onchain_balance = Amount::try_from(info.onchain_balance.total())
            .expect("We're unreasonably rich!");
        let trusted_onchain_balance =
            Amount::try_from(info.onchain_balance.trusted_spendable())
                .expect("We're unreasonably rich!");
        let balance = lightning_balance.saturating_add(onchain_balance);

        Self {
            version: info.version,
            measurement: info.measurement,
            user_pk: info.user_pk,
            node_pk: info.node_pk,

            balance,

            lightning_balance,
            usable_lightning_balance: info.lightning_balance.usable,
            max_sendable_lightning_balance: info.lightning_balance.max_sendable,
            onchain_balance,
            trusted_onchain_balance,
            num_channels: info.num_channels,
            num_usable_channels: info.num_usable_channels,
        }
    }
}

/// A request to create a BOLT 11 invoice.
#[derive(Default, Serialize, Deserialize)]
pub struct SdkCreateInvoiceRequest {
    /// The expiration, in seconds, to encode into the invoice.
    pub expiration_secs: u32,

    /// Optionally include an amount, in sats, to encode into the invoice.
    /// If no amount is provided, the sender will specify how much to pay.
    pub amount: Option<Amount>,

    /// The description to be encoded into the invoice.
    /// The sender will see this description when they scan the invoice.
    // If `None`, the `description` field inside the invoice will be an empty
    // string (""), as lightning _requires_ a description (or description
    // hash) to be set.
    pub description: Option<String>,
}

/// The response to a BOLT 11 invoice request.
#[derive(Serialize, Deserialize)]
pub struct SdkCreateInvoiceResponse {
    /// Identifier for this inbound invoice payment.
    pub index: PaymentIndex,
    /// The string-encoded BOLT 11 invoice.
    pub invoice: LxInvoice,
    /// The description encoded in the invoice, if one was provided.
    pub description: Option<String>,
    /// The amount encoded in the invoice, if there was one.
    /// Returning `null` means we created an amountless invoice.
    pub amount: Option<Amount>,
    /// The invoice creation time, in milliseconds since the UNIX epoch.
    pub created_at: TimestampMs,
    /// The invoice expiration time, in milliseconds since the UNIX epoch.
    pub expires_at: TimestampMs,
    /// The hex-encoded payment hash of the invoice.
    pub payment_hash: LxPaymentHash,
    /// The payment secret of the invoice.
    pub payment_secret: LxPaymentSecret,
}

impl SdkCreateInvoiceResponse {
    /// Quickly create a `SdkCreateInvoiceResponse`
    pub fn new(index: PaymentIndex, invoice: LxInvoice) -> Self {
        let description = invoice.description_str().map(|s| s.to_owned());
        let amount_sats = invoice.amount();
        let created_at = invoice.saturating_created_at();
        let expires_at = invoice.saturating_expires_at();
        let payment_hash = invoice.payment_hash();
        let payment_secret = invoice.payment_secret();

        Self {
            index,
            invoice,
            description,
            amount: amount_sats,
            created_at,
            expires_at,
            payment_hash,
            payment_secret,
        }
    }
}

impl From<SdkCreateInvoiceRequest> for command::CreateInvoiceRequest {
    fn from(sdk: SdkCreateInvoiceRequest) -> Self {
        Self {
            expiry_secs: sdk.expiration_secs,
            amount: sdk.amount,
            description: sdk.description,
        }
    }
}

/// A request to pay a BOLT 11 invoice.
#[derive(Serialize, Deserialize)]
pub struct SdkPayInvoiceRequest {
    /// The invoice we want to pay, as a string.
    pub invoice: LxInvoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be set if the invoice is amountless.
    pub fallback_amount: Option<Amount>,
    /// An optional personal note for this payment.
    /// The receiver will not see this note.
    pub note: Option<String>,
}

impl From<SdkPayInvoiceRequest> for command::PayInvoiceRequest {
    fn from(sdk: SdkPayInvoiceRequest) -> Self {
        Self {
            invoice: sdk.invoice,
            fallback_amount: sdk.fallback_amount,
            note: sdk.note,
        }
    }
}

/// The response to a request to pay a BOLT 11 invoice.
#[derive(Serialize, Deserialize)]
pub struct SdkPayInvoiceResponse {
    /// Identifier for this outbound invoice payment.
    pub index: PaymentIndex,
    /// When we tried to pay this invoice, in milliseconds since the UNIX
    /// epoch.
    pub created_at: TimestampMs,
}

/// A request to get information about a payment by its index.
#[derive(Serialize, Deserialize)]
pub struct SdkGetPaymentRequest {
    /// Identifier for this payment.
    pub index: PaymentIndex,
}

/// A response to a request to get information about a payment by its index.
#[derive(Serialize, Deserialize)]
pub struct SdkGetPaymentResponse {
    /// Information about this payment, if it exists.
    pub payment: Option<SdkPayment>,
}
