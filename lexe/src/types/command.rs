//! Lexe SDK API request and response types.

use anyhow::Context;
use lexe_api::{
    models::command,
    types::{
        bounded_note::BoundedNote,
        invoice::Invoice,
        payments::{PaymentCreatedIndex, PaymentHash, PaymentSecret},
    },
};
use lexe_common::{ln::amount::Amount, time::TimestampMs};
use serde::{Deserialize, Serialize};

use crate::types::{
    auth::{Measurement, NodePk, UserPk},
    payment::Payment,
};

/// Information about a Lexe node.
// Simple version of `lexe_api::models::command::NodeInfo`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    /// The node's current semver version, e.g. `0.6.9`.
    pub version: semver::Version,
    /// The hex-encoded SGX 'measurement' of the current node.
    /// The measurement is the hash of the enclave binary.
    pub measurement: Measurement,
    /// The hex-encoded ed25519 user public key used to identify a Lexe user.
    /// The user keypair is derived from the root seed.
    pub user_pk: UserPk,
    /// The hex-encoded secp256k1 Lightning node public key; the `node_id`.
    pub node_pk: NodePk,

    /// The sum of our `lightning_balance` and our `onchain_balance`, in sats.
    pub balance: Amount,

    /// Total Lightning balance in sats, summed over all of our channels.
    pub lightning_balance: Amount,
    /// An estimated upper bound, in sats, on how much of our Lightning balance
    /// we can send to most recipients on the Lightning Network, accounting for
    /// Lightning limits such as our channel reserve, pending HTLCs, fees, etc.
    /// You should usually be able to spend this amount.
    // User-facing name for `LightningBalance::sendable`
    pub lightning_sendable_balance: Amount,
    /// A hard upper bound on how much of our Lightning balance can be spent
    /// right now, in sats. This is always >= `lightning_sendable_balance`.
    /// Generally it is only possible to spend exactly this amount if the
    /// recipient is a Lexe user.
    // User-facing name for `LightningBalance::max_sendable`
    pub lightning_max_sendable_balance: Amount,

    /// Total on-chain balance in sats, including unconfirmed funds.
    // `OnchainBalance::total`
    pub onchain_balance: Amount,
    /// Trusted on-chain balance in sats, including only confirmed funds and
    /// unconfirmed outputs originating from our own wallet.
    // Equivalent to BDK's `trusted_spendable`, but with a better name.
    pub onchain_trusted_balance: Amount,

    /// The total number of Lightning channels.
    pub num_channels: usize,
    /// The number of channels which are currently usable, i.e. `channel_ready`
    /// messages have been exchanged and the channel peer is online.
    /// Is always less than or equal to `num_channels`.
    pub num_usable_channels: usize,
}

impl From<command::NodeInfo> for NodeInfo {
    fn from(info: command::NodeInfo) -> Self {
        let lightning_balance = info.lightning_balance.total();
        let onchain_balance = Amount::try_from(info.onchain_balance.total())
            .expect("We're unreasonably rich!");
        let onchain_trusted_balance =
            Amount::try_from(info.onchain_balance.trusted_spendable())
                .expect("We're unreasonably rich!");
        let balance = lightning_balance.saturating_add(onchain_balance);

        Self {
            version: info.version,
            measurement: Measurement::from_unstable(info.measurement),
            user_pk: UserPk::from_unstable(info.user_pk),
            node_pk: NodePk::from_unstable(info.node_pk),

            balance,

            lightning_balance,
            lightning_sendable_balance: info.lightning_balance.sendable,
            lightning_max_sendable_balance: info.lightning_balance.max_sendable,
            onchain_balance,
            onchain_trusted_balance,
            num_channels: info.num_channels,
            num_usable_channels: info.num_usable_channels,
        }
    }
}

/// A request to create a BOLT 11 invoice.
#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
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

    /// An optional note received from the payer out-of-band via LNURL-pay
    /// that is stored with this inbound payment. If provided, it must be
    /// non-empty and no longer than 200 chars / 512 UTF-8 bytes.
    #[serde(default)]
    pub payer_note: Option<String>,
}

/// The response to a BOLT 11 invoice request.
#[derive(Serialize, Deserialize)]
pub struct CreateInvoiceResponse {
    /// Identifier for this inbound invoice payment.
    pub index: PaymentCreatedIndex,
    /// The string-encoded BOLT 11 invoice.
    pub invoice: Invoice,
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
    pub payment_hash: PaymentHash,
    /// The payment secret of the invoice.
    pub payment_secret: PaymentSecret,
}

impl CreateInvoiceResponse {
    /// Build a [`CreateInvoiceResponse`] from an index and invoice.
    pub fn new(index: PaymentCreatedIndex, invoice: Invoice) -> Self {
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

impl TryFrom<CreateInvoiceRequest> for command::CreateInvoiceRequest {
    type Error = anyhow::Error;

    fn try_from(req: CreateInvoiceRequest) -> anyhow::Result<Self> {
        Ok(Self {
            expiry_secs: req.expiration_secs,
            amount: req.amount,
            description: req.description,
            // TODO(maurice): Add description_hash if we really need it.
            description_hash: None,
            payer_note: req
                .payer_note
                .map(BoundedNote::new)
                .transpose()
                .context(
                    "Invalid payer_note (must be non-empty and <=200 chars / \
                     <=512 UTF-8 bytes)",
                )?,
        })
    }
}

/// A request to pay a BOLT 11 invoice.
#[derive(Serialize, Deserialize)]
pub struct PayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: Invoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be set if the invoice is amountless.
    pub fallback_amount: Option<Amount>,
    /// An optional personal note for this payment.
    /// The receiver will not see this note.
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub note: Option<String>,
    /// An optional note that was sent to the receiver out-of-band via
    /// LNURL-pay that is stored with this outbound payment. Unlike `note`,
    /// this is visible to the recipient. If provided, it must be non-empty and
    /// no longer than 200 chars / 512 UTF-8 bytes.
    pub payer_note: Option<String>,
}

impl TryFrom<PayInvoiceRequest> for command::PayInvoiceRequest {
    type Error = anyhow::Error;

    fn try_from(req: PayInvoiceRequest) -> anyhow::Result<Self> {
        Ok(Self {
            invoice: req.invoice,
            fallback_amount: req.fallback_amount,
            note: req.note.map(BoundedNote::new).transpose().context(
                "Invalid note (must be non-empty and <=200 chars / \
                     <=512 UTF-8 bytes)",
            )?,
            payer_note: req
                .payer_note
                .map(BoundedNote::new)
                .transpose()
                .context(
                    "Invalid payer_note (must be non-empty and <=200 chars / \
                     <=512 UTF-8 bytes)",
                )?,
        })
    }
}

/// The response to a request to pay a BOLT 11 invoice.
#[derive(Serialize, Deserialize)]
pub struct PayInvoiceResponse {
    /// Identifier for this outbound invoice payment.
    pub index: PaymentCreatedIndex,
    /// When we tried to pay this invoice, in milliseconds since the UNIX
    /// epoch.
    pub created_at: TimestampMs,
}

/// A request to update the personal note on an existing payment.
/// Pass `None` to clear the note.
#[derive(Serialize, Deserialize)]
pub struct UpdatePaymentNoteRequest {
    /// Identifier for the payment to be updated.
    pub index: PaymentCreatedIndex,
    /// The updated note, or `None` to clear.
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub note: Option<String>,
}

impl TryFrom<UpdatePaymentNoteRequest> for command::UpdatePaymentNote {
    type Error = anyhow::Error;

    fn try_from(sdk: UpdatePaymentNoteRequest) -> anyhow::Result<Self> {
        Ok(Self {
            index: sdk.index,
            note: sdk.note.map(BoundedNote::new).transpose().context(
                "Invalid note (must be non-empty and <=200 chars / \
                 <=512 UTF-8 bytes)",
            )?,
        })
    }
}

/// A request to get information about a payment by its index.
#[derive(Serialize, Deserialize)]
pub struct GetPaymentRequest {
    /// Identifier for this payment.
    pub index: PaymentCreatedIndex,
}

/// A response to a request to get information about a payment by its index.
#[derive(Serialize, Deserialize)]
pub struct GetPaymentResponse {
    /// Information about this payment, if it exists.
    pub payment: Option<Payment>,
}

/// Response from listing payments.
#[derive(Serialize, Deserialize)]
pub struct ListPaymentsResponse {
    /// Payments in the requested page.
    pub payments: Vec<Payment>,
    /// Cursor for fetching the next page. `None` when there are no more
    /// results. Pass this as the `after` argument to get the next page.
    pub next_index: Option<PaymentCreatedIndex>,
}

/// Summary of changes from a payment sync operation.
#[derive(Debug)]
pub struct PaymentSyncSummary {
    /// Number of new payments added to the local database.
    pub num_new: usize,
    /// Number of existing payments that were updated.
    pub num_updated: usize,
}
