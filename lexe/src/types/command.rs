//! Lexe SDK API request and response types.

use std::collections::HashMap;

use anyhow::Context;
use lexe_api::{
    models::command,
    types::{
        bounded_string::BoundedString,
        invoice::Invoice,
        lnurl::LnurlPayRequest,
        payments::{
            ClientPaymentId, PaymentCreatedIndex, PaymentHash, PaymentSecret,
            PaymentUpdatedIndex,
        },
    },
};
use lexe_common::{
    api::{auth::LexeScope, revocable_clients},
    constants,
    ln::amount::Amount,
    ppm::Ppm,
    time::TimestampMs,
};
use lexe_payment_uri::{ClaimMethod, LnurlWithdrawRequest, PaymentMethod};
use lexe_std::const_assert_usize_eq;
use serde::{Deserialize, Serialize};

use crate::{
    types::{
        auth::{ClientCredentials, Measurement, NodePk, UserPk},
        bitcoin::Offer,
        payment::Payment,
    },
    util::ed25519,
};

// --- Node management --- //

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

// --- Paying and receiving Bitcoin --- //

/// A request to analyze the contents of a Bitcoin or Lightning payment string.
/// Reveals all payment methods encoded in the string, and gives payment-related
/// details on each. See [`PayableDetails`] for more info.
#[derive(Serialize, Deserialize)]
pub struct AnalyzeRequest {
    /// The Bitcoin or Lightning payment string to analyze.
    pub payment_string: String,
}

/// The response to a string analysis request.
pub struct AnalyzeResponse {
    /// The valid payment routes encoded in the analyzed string, ordered
    /// by most recommended payment route first, and least recommended payment
    /// route last.
    ///
    /// "Payable" indicates an outbound payment flow.
    pub payables: Vec<PayableDetails>,

    /// The valid claim routes encoded in the analyzed string.
    ///
    /// "Claimable" indicates an inbound payment flow.
    pub claimables: Vec<ClaimableDetails>,
}

/// Describes basic information for a payable string.
pub struct PayableDetails {
    /// The payable string encoding the payment method.
    pub payable: String,
    /// The deserialized payment method.
    pub method: PaymentMethod,

    /// The description encoded in the `payable`, if any.
    pub description: Option<String>,

    /// The amount that should be paid to the `payable`; if `None`, the payer
    /// should specify an amount to pay.
    ///
    /// This will be `None` if `min_amount` or `max_amount` are specified.
    pub amount: Option<Amount>,
    /// The minimum amount that can be paid to the `payable`.
    ///
    /// This will be `None` if `amount` is specified.
    pub min_amount: Option<Amount>,
    /// The maximum amount that can be paid to the `payable`.
    ///
    /// This will be `None` if `amount` is specified.
    pub max_amount: Option<Amount>,

    /// The payable expiration time, in milliseconds since the UNIX epoch.
    pub expires_at: Option<TimestampMs>,
}

/// Describes basic information for a claimable string.
pub struct ClaimableDetails {
    /// The claimable string encoding the claim method.
    pub claimable: String,
    /// The deserialized claim method.
    pub method: ClaimMethod,

    /// The description encoded in the `claimable`, if any.
    pub description: Option<String>,

    // /// The amount that should be received from the `claimable`; if `None`,
    // /// the claimer should specify an amount to claim.
    // ///
    // /// This will be `None` if `min_amount` or `max_amount` are specified.
    // pub amount: Option<Amount>,
    ///
    /// The minimum amount that can be received from the `claimable`.
    ///
    /// This will be `None` if `amount` is specified.
    pub min_amount: Option<Amount>,
    /// The maximum amount that can be received from the `claimable`.
    ///
    /// This will be `None` if `amount` is specified.
    pub max_amount: Option<Amount>,
    //
    // /// The claimable expiration time, in milliseconds since the UNIX epoch.
    // pub expires_at: Option<TimestampMs>,
}

/// A catch-all request to pay a Bitcoin or Lightning payment string.
///
/// The following encodings are supported:
///   - BIP 321 URI: `bitcoin:bc1...`
///   - Lightning URI: `lightning:ln...`
///   - BOLT 11 invoice: `lnbc1...`
///   - BOLT 12 offer: `lno1...`
///   - Onchain bitcoin address: `bc1...`
///   - Human Bitcoin Address: `₿satoshi@lexe.app`
///   - Lightning Address: `satoshi@lexe.app`
///   - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
///
/// See [`PaymentMethod`] for more details on supported payment methods.
///
/// If there exist multiple encoded payment methods, the best recommended
/// payment method will be chosen.
#[derive(Serialize, Deserialize)]
pub struct PayRequest {
    /// The string we will pay.
    pub payable: String,

    /// The amount we will attempt to pay.
    /// If the payable specifies an amount, this field is optional.
    pub amount: Option<Amount>,

    /// An optional message to send to the recipient, supported if sending to:
    ///
    /// - BOLT 12 offers
    /// - Human Bitcoin Addresses which point to an offer
    /// - LNURL recipients whose wallets accept LUD-12 comments
    /// - Lightning Addresses to a wallet that accepts LUD-12 comments
    ///
    /// If the payable doesn't support messages, this will be ignored.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars / 512
    /// UTF-8 bytes.
    pub message: Option<String>,

    /// An optional personal note for this payment.
    ///
    /// The receiver will not see this note.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub personal_note: Option<String>,
}

/// A request to create a BOLT 11 invoice.
#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    /// The expiration, in seconds, to encode into the invoice.
    /// If no duration is provided, the expiration time defaults to 86400
    /// (1 day).
    pub expiration_secs: Option<u32>,

    /// Optionally include an amount, in sats, to encode into the invoice.
    /// If no amount is provided, the payer will specify how much to pay.
    pub amount: Option<Amount>,

    /// The description to be encoded into the invoice.
    /// The payer will see this description when they scan the invoice.
    // If `None`, the `description` field inside the invoice will be an empty
    // string (""), as lightning _requires_ a description (or description
    // hash) to be set.
    pub description: Option<String>,

    /// An optional personal note for this invoice.
    ///
    /// The payer will not see this note.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    // Added in `node-v0.9.10`
    pub personal_note: Option<String>,

    /// The partner's user_pk, if the partner is setting the fee for this
    /// payment instead of using Lexe's default fees.
    ///
    /// This must be set in order for `partner_prop_fee` and `partner_base_fee`
    /// to take effect.
    // Added in `node-v0.9.6`
    pub partner_pk: Option<UserPk>,

    /// The partner-chosen proportional fee to charge on this payment.
    /// If `partner_pk` is set, this must be set to [`Some`].
    ///
    /// Minimum: 5000 ppm (`LSP_USERNODE_SKIM_FEE`)
    /// Maximum: 500,000 ppm (50%)
    // Added in `node-v0.9.6`
    pub partner_prop_fee: Option<Ppm>,

    /// The partner-chosen base fee to charge on this payment.
    ///
    /// If this is set, the invoice `amount` must also be set.
    // Added in `node-v0.9.6`
    pub partner_base_fee: Option<Amount>,
}

/// The response to a BOLT 11 invoice request.
#[derive(Serialize, Deserialize)]
pub struct CreateInvoiceResponse {
    /// Identifier for this inbound invoice payment.
    pub index: PaymentCreatedIndex,
    /// The BOLT 11 invoice.
    pub invoice: Invoice,
    /// The description encoded in the invoice, if one was provided.
    pub description: Option<String>,
    /// The amount encoded in the invoice, if there was one.
    /// Returning `None` means we created an amountless invoice.
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
        /// The default expiration we use if none is provided.
        const DEFAULT_EXPIRATION_SECS: u32 = 60 * 60 * 24; // 1 day

        Ok(Self {
            expiry_secs: req.expiration_secs.unwrap_or(DEFAULT_EXPIRATION_SECS),
            amount: req.amount,
            description: req.description,
            // TODO(maurice): Add description_hash if we really need it.
            description_hash: None,
            message: None,
            personal_note: req
                .personal_note
                .map(BoundedString::new)
                .transpose()?,
            partner_pk: req.partner_pk.map(|pk| pk.unstable()),
            partner_prop_fee: req.partner_prop_fee,
            partner_base_fee: req.partner_base_fee,
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
    pub personal_note: Option<String>,
}

impl TryFrom<PayInvoiceRequest> for command::PayInvoiceRequest {
    type Error = anyhow::Error;

    fn try_from(req: PayInvoiceRequest) -> anyhow::Result<Self> {
        Ok(Self {
            invoice: req.invoice,
            fallback_amount: req.fallback_amount,
            message: None,
            personal_note: req
                .personal_note
                .map(BoundedString::new)
                .transpose()
                .context("Invalid personal note")?,
        })
    }
}

/// A request to create a BOLT 12 offer to receive Lightning payments.
///
/// Unlike invoices, offers are reusable: multiple payments can be made to
/// it, including from multiple payers.
#[derive(Default, Serialize, Deserialize)]
pub struct CreateOfferRequest {
    /// An optional description to encode into the offer.
    ///
    /// The sender will see this description when they scan the offer.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub description: Option<String>,

    /// An optional minimum payment size for payments to this offer.
    /// If not set, the payer can send any amount.
    pub min_amount: Option<Amount>,

    /// An optional expiration for the offer, in seconds from now.
    pub expiration_secs: Option<u32>,
}

impl TryFrom<CreateOfferRequest> for command::CreateOfferRequest {
    type Error = anyhow::Error;

    fn try_from(req: CreateOfferRequest) -> anyhow::Result<Self> {
        let description = req
            .description
            .map(BoundedString::new)
            .transpose()
            .context("Invalid description")?;

        Ok(Self {
            description,
            min_amount: req.min_amount,
            expiry_secs: req.expiration_secs,
            max_quantity: None,
            issuer: None,
        })
    }
}

/// The response to a BOLT 12 offer creation request.
#[derive(Serialize, Deserialize)]
pub struct CreateOfferResponse {
    /// The BOLT 12 offer.
    pub offer: Offer,
}

/// A request to pay a BOLT 12 offer over Lightning.
#[derive(Serialize, Deserialize)]
pub struct PayOfferRequest {
    /// The offer we want to pay.
    pub offer: Offer,
    /// The amount we will pay. If the offer specifies a minimum amount,
    /// this value must satisfy that minimum.
    pub amount: Amount,
    /// An optional message (sent as a BOLT 12 `payer_note`) included with the
    /// invoice request and visible to the recipient. If provided, it must be
    /// non-empty and no longer than 200 chars / 512 UTF-8 bytes.
    pub message: Option<String>,
    /// An optional personal note for this payment.
    /// The receiver will not see this note.
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub personal_note: Option<String>,
}

impl PayOfferRequest {
    /// Build a [`command::PayOfferRequest`] from this SDK request.
    pub(crate) fn into_unstable(
        self,
        cid: ClientPaymentId,
    ) -> anyhow::Result<command::PayOfferRequest> {
        Ok(command::PayOfferRequest {
            cid,
            offer: self.offer,
            amount: self.amount,
            message: self
                .message
                .map(BoundedString::new)
                .transpose()
                .context("Invalid message")?,
            personal_note: self
                .personal_note
                .map(BoundedString::new)
                .transpose()
                .context("Invalid personal note")?,
        })
    }
}

/// A request to pay to an LNURL-pay endpoint.
///
/// Use [`analyze`] to get the associated [`LnurlPayRequest`], which
/// contains information on amount constraints, message limits, and more.
///
/// [`analyze`]: crate::wallet::LexeWallet::analyze
pub struct PayLnurlRequest {
    /// The LNURL or Lightning Address to pay to.
    ///
    /// Exactly one of `lnurl` or `pay_request` should be provided.
    pub lnurl: Option<String>,
    /// The LNURL pay request to use.
    ///
    /// Exactly one of `lnurl` or `pay_request` should be provided.
    pub pay_request: Option<LnurlPayRequest>,
    /// The amount to pay. This value must satisfy the minimum and maximum
    /// limits set by the LNURL endpoint.
    pub amount: Amount,
    /// An optional message to include in the payment,
    /// visible to the recipient.
    ///
    /// Will only be sent if the LNURL endpoint supports it, and will be
    /// truncated to the LNURL endpoint's specified length limits if needed.
    pub message: Option<String>,
    /// An optional personal note for this payment.
    /// The receiver will not see this note.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub personal_note: Option<String>,
}

/// A request to withdraw from an LNURL-withdraw endpoint.
///
/// Use [`analyze`] to get the associated [`LnurlWithdrawRequest`], which
/// contains information on amount constraints, default description, and more.
///
/// [`analyze`]: crate::wallet::LexeWallet::analyze
pub struct WithdrawLnurlRequest {
    /// The LNURL to withdraw from.
    ///
    /// Exactly one of `lnurl` or `withdraw_request` should be provided.
    pub lnurl: Option<String>,
    /// The LNURL withdraw request to use.
    ///
    /// Exactly one of `lnurl` or `withdraw_request` should be provided.
    pub withdraw_request: Option<LnurlWithdrawRequest>,
    /// The amount to withdraw. This value must satisfy the minimum and maximum
    /// limits set by the LNURL endpoint.
    ///
    /// If `None`, the maximum amount will be withdrawn.
    pub amount: Option<Amount>,
    /// An optional description to encode into the withdrawal invoice,
    /// visible to the LNURL endpoint.
    ///
    /// If `None`, the description encoded will be the default description
    /// specified by the LNURL endpoint, if any.
    pub description: Option<String>,
    /// An optional personal note for this withdrawal.
    ///
    /// The LNURL endpoint will not see this note.
    ///
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    pub personal_note: Option<String>,
}

// --- Payment information and management --- //

/// Summary of changes from a payment sync operation.
#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentSyncSummary {
    /// Number of new payments added to the local database.
    pub num_new: usize,
    /// Number of existing payments that were updated.
    pub num_updated: usize,
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

/// Get a batch of payments in ascending `updated_at` order, starting from
/// a given `updated_at` index.
///
/// Useful for tailing / syncing payment updates as they occur and merging them
/// into a local payments store.
#[derive(Serialize, Deserialize)]
pub struct GetUpdatedPaymentsRequest {
    /// The cursor at which the results should start, exclusive.
    ///
    /// Payments that were last updated earlier than or equal to this will not
    /// be returned.
    ///
    /// If `None`, the least recently updated payments will be returned first.
    pub start_index: Option<PaymentUpdatedIndex>,
    /// The maximum number of payments that can be returned.
    ///
    /// Maximum value: 100. Defaults to 50 if not set.
    pub limit: Option<u16>,
}

// If either of these break, update the docs above.
const_assert_usize_eq!(constants::MAX_PAYMENTS_BATCH_SIZE as usize, 100);
const_assert_usize_eq!(constants::DEFAULT_PAYMENTS_BATCH_SIZE as usize, 50);

/// A response to a [`GetUpdatedPaymentsRequest`].
#[derive(Serialize, Deserialize)]
pub struct GetUpdatedPaymentsResponse {
    /// The updated payments, in ascending [`PaymentUpdatedIndex`] order.
    pub payments: Vec<Payment>,
    /// The `updated_at` index of the last payment in the returned batch.
    ///
    /// To continue syncing, pass this as `start_index` in the next request.
    pub updated_index: Option<PaymentUpdatedIndex>,
}

/// A request to update the personal note on an existing payment.
/// Pass `None` to clear the note.
#[derive(Serialize, Deserialize)]
pub struct UpdatePersonalNoteRequest {
    /// Identifier for the payment to be updated.
    pub index: PaymentCreatedIndex,
    /// The updated note, or `None` to clear.
    /// If provided, it must be non-empty and no longer than 200 chars /
    /// 512 UTF-8 bytes.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "note", alias = "personal_note")]
    pub personal_note: Option<String>,
}

impl TryFrom<UpdatePersonalNoteRequest> for command::UpdatePersonalNote {
    type Error = anyhow::Error;

    fn try_from(sdk: UpdatePersonalNoteRequest) -> anyhow::Result<Self> {
        Ok(Self {
            index: sdk.index,
            personal_note: sdk
                .personal_note
                .map(BoundedString::new)
                .transpose()
                .context("Invalid note")?,
        })
    }
}

// --- Client credentials management --- //

/// Information about a client that can authenticate with a Lexe node.
#[derive(Serialize)]
pub struct ClientInfo {
    /// The public key of the client.
    pub client_pk: ed25519::PublicKey,
    /// The time at which the client was created,
    /// in milliseconds since the UNIX epoch.
    pub created_at: TimestampMs,
    /// The time at which the client expires,
    /// in milliseconds since the UNIX epoch.
    ///
    /// [`None`] means that the client will never expire.
    pub expires_at: Option<TimestampMs>,
    /// An optional label for the client.
    pub label: Option<String>,
    // TODO(nicole): Add the application scope when it's useful
    // scope: Scope,
}

impl From<revocable_clients::RevocableClient> for ClientInfo {
    fn from(value: revocable_clients::RevocableClient) -> Self {
        Self {
            client_pk: value.pubkey,
            created_at: value.created_at,
            expires_at: value.expires_at,
            label: value.label,
        }
    }
}

/// A response containing information about a single client.
#[derive(Serialize)]
pub struct ClientInfoResponse {
    /// Information about the client.
    pub client: ClientInfo,
}

/// The response to a request to get information about active clients
/// on a Lexe node.
#[derive(Serialize)]
pub struct GetClientResponse {
    /// The clients that can authenticate with this node, mapped from
    /// client public key to client information.
    pub clients: HashMap<ed25519::PublicKey, ClientInfo>,
}

/// A request to create a new client and client credentials
/// that can authenticate with a Lexe node.
pub struct CreateClientRequest {
    /// An optional expiration for the client.
    ///
    /// [`None`] indicates that the client should never expire. Use carefully!
    pub expires_at: Option<TimestampMs>,
    /// An optional label for the client.
    ///
    /// Must be less than 64 UTF-8 bytes if provided.
    pub label: Option<String>,
    // TODO(nicole): Add scope when it's useful
    // pub scope: LexeScope,
}

// If this breaks, update the docs above.
const_assert_usize_eq!(revocable_clients::RevocableClient::MAX_LABEL_LEN, 64);

impl From<CreateClientRequest>
    for revocable_clients::CreateRevocableClientRequest
{
    fn from(req: CreateClientRequest) -> Self {
        Self {
            expires_at: req.expires_at,
            label: req.label,
            // TODO(nicole): Allow configuring scope when it becomes useful
            scope: LexeScope::All,
        }
    }
}

/// The response to a request to create a new client.
pub struct CreateClientResponse {
    /// The public key of the created client.
    pub client_pk: ed25519::PublicKey,
    /// The client credentials that can be used to authenticate as this client.
    pub client_credentials: ClientCredentials,
    /// The time at which the client was created,
    /// in milliseconds since the UNIX epoch.
    pub created_at: TimestampMs,
}

/// A request to update the properties of an existing client.
pub struct UpdateClientRequest {
    /// The public key of the client to update.
    pub client_pk: ed25519::PublicKey,
    /// The updated label for the client.
    ///
    /// - `None`: leave the label unchanged.
    /// - `Some(None)`: clear the label.
    /// - `Some(Some(label))`: set the label.
    pub new_label: Option<Option<String>>,
    /// The updated expiration for the client.
    ///
    /// - `None`: leave the expiration unchanged.
    /// - `Some(None)`: make the client never expire. Use carefully!
    /// - `Some(Some(expires_at))`: set the expiration.
    pub new_expires_at: Option<Option<TimestampMs>>,
}

impl From<UpdateClientRequest> for revocable_clients::UpdateClientRequest {
    fn from(req: UpdateClientRequest) -> Self {
        Self {
            pubkey: req.client_pk,
            expires_at: req.new_expires_at,
            label: req.new_label,
            scope: None,
            is_revoked: None,
        }
    }
}

/// A request to permanently revoke a client, making its credentials invalid for
/// authentication.
pub struct RevokeClientRequest {
    /// The public key of the client to revoke.
    pub client_pk: ed25519::PublicKey,
}
