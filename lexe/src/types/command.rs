//! Lexe SDK API request and response types.

use std::collections::HashMap;

use anyhow::{Context, ensure};
use lexe_api::{
    models::command,
    revocable_clients,
    types::{
        bounded_string::BoundedString,
        invoice::Invoice,
        lnurl::LnurlPayRequest,
        payments::{
            ClientPaymentId, PaymentCreatedIndex, PaymentHash, PaymentKind,
            PaymentSecret, PaymentUpdatedIndex,
        },
    },
};
use lexe_common::{
    api::auth::LexeScope,
    constants,
    ln::{amount::Amount, channel::LxChannelDetails},
    ppm::Ppm,
    time::TimestampMs,
};
use lexe_payment_uri::{ClaimMethod, LnurlWithdrawRequest, PaymentMethod};
use lexe_std::const_assert_usize_eq;
use serde::{Deserialize, Serialize};

use crate::{
    types::{
        auth::{ClientCredentials, Measurement, NodePk, UserPk},
        bitcoin::{ChannelId, Offer, OutPoint, UserChannelId},
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

    /// The sum of our `lightning_balance` and our `onchain_balance`.
    pub balance: Amount,

    /// Total Lightning balance, summed over all of our channels.
    pub lightning_balance: Amount,
    /// An estimated upper bound on how much of our Lightning balance
    /// we can send to most recipients on the Lightning Network, accounting for
    /// Lightning limits such as our channel reserve, pending HTLCs, fees, etc.
    /// You should usually be able to spend this amount.
    // User-facing name for `LightningBalance::sendable`
    pub lightning_sendable_balance: Amount,
    /// A hard upper bound on how much of our Lightning balance can be spent
    /// right now. This is always >= `lightning_sendable_balance`.
    /// Generally it is only possible to spend exactly this amount if the
    /// recipient is a Lexe user.
    // User-facing name for `LightningBalance::max_sendable`
    pub lightning_max_sendable_balance: Amount,

    /// Total on-chain balance, including unconfirmed funds.
    // `OnchainBalance::total`
    pub onchain_balance: Amount,
    /// Trusted on-chain balance, including only confirmed funds and
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

    /// Optionally include an amount to encode into the invoice.
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
            // We intentionally do not expose the payment kind in the Lexe SDK.
            kind: PaymentKind::Invoice,
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
            // We intentionally do not expose the payment kind in the Lexe SDK.
            kind: PaymentKind::Invoice,
            // TODO(nicole): expose preflight endpoints
            ldk_route: None,
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
            // We intentionally do not expose the payment kind in the Lexe SDK.
            kind: PaymentKind::Offer,
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

/// A request to buy Bitcoin with Cash App.
///
/// See [`buy_with_cash_app`](crate::wallet::LexeWallet::buy_with_cash_app).
#[derive(Serialize, Deserialize)]
pub struct CashAppBuyRequest {
    /// The amount of Bitcoin to buy, in sats. Must be at least 5000 sats.
    pub amount: Amount,
}

/// The response to a [`CashAppBuyRequest`].
#[derive(Serialize, Deserialize)]
pub struct CashAppBuyResponse {
    /// A Cash App URL that funds the purchase.
    ///
    /// Redirect your user to this URL to complete the purchase; for the
    /// smoothest experience, have them open it on a device where Cash App is
    /// already set up. The bought Bitcoin lands directly into Lexe wallet.
    pub redirect_url: String,
    /// Identifier for the inbound payment funding this buy. Use it to look up
    /// the payment (e.g. `get_payment`) once Cash App has funded it.
    pub index: PaymentCreatedIndex,
}

/// The user's Human Bitcoin Address.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GetHumanBitcoinAddressResponse {
    /// The Human Bitcoin Address (BIP 353), e.g. `₿satoshi@lexe.app`.
    pub human_bitcoin_address: String,
    /// The Lightning Address, e.g. `satoshi@lexe.app`.
    pub lightning_address: String,
    /// The BOLT 12 offer that the Human Bitcoin Address resolves to.
    pub offer: Offer,
    /// Whether the username can currently be changed. Usernames are
    /// updatable for 24 hours after being claimed, then frozen for 90 days.
    pub updatable: bool,
}

impl From<command::ActiveHumanBitcoinAddress>
    for GetHumanBitcoinAddressResponse
{
    fn from(active: command::ActiveHumanBitcoinAddress) -> Self {
        Self {
            human_bitcoin_address: active.hba.username.human_bitcoin_address(),
            lightning_address: active.hba.username.lightning_address(),
            offer: active.hba.offer,
            updatable: active.updatable,
        }
    }
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

// --- Channel management --- //

/// Details about one of this node's Lightning channels.
#[derive(Serialize, Deserialize)]
pub struct ChannelDetails {
    /// The id of the channel.
    pub channel_id: ChannelId,
    /// A user-provided id for this channel that's associated with the channel
    /// throughout its whole lifetime, as the Lightning protocol channel id is
    /// only known after negotiating the channel and creating the funding tx.
    pub user_channel_id: UserChannelId,
    /// The channel's funding transaction output, or `None` if the funding
    /// transaction has not yet been confirmed.
    pub funding_txo: Option<OutPoint>,
    /// Whether the channel is ready and its counterparty is online, so it can
    /// send and receive payments right now.
    pub is_usable: bool,

    /// The total value of the channel.
    pub channel_value: Amount,
    /// Our balance in the channel.
    pub our_balance: Amount,
    /// The counterparty's balance in the channel.
    pub their_balance: Amount,
    /// The portion of our balance that the counterparty requires us to keep in
    /// reserve as anti-cheating collateral. This is unspendable and does not
    /// count towards `outbound_capacity`.
    pub punishment_reserve: Amount,

    /// How much of our balance is currently available to send.
    pub outbound_capacity: Amount,
    /// How much of the counterparty's balance is available for us to receive.
    pub inbound_capacity: Amount,
}

impl From<LxChannelDetails> for ChannelDetails {
    fn from(details: LxChannelDetails) -> Self {
        Self {
            channel_id: details.channel_id,
            user_channel_id: details.user_channel_id,
            funding_txo: details.funding_txo,
            is_usable: details.is_usable,
            channel_value: details.channel_value,
            our_balance: details.our_balance,
            their_balance: details.their_balance,
            punishment_reserve: details.punishment_reserve,
            outbound_capacity: details.outbound_capacity,
            inbound_capacity: details.inbound_capacity,
        }
    }
}

/// The response to a request to list this node's Lightning channels.
///
/// All of this node's Lightning channels are connected to the Lexe LSP.
#[derive(Serialize, Deserialize)]
pub struct ListChannelsResponse {
    /// This node's Lightning channels. The counterparty is always the Lexe
    /// LSP.
    pub channels: Vec<ChannelDetails>,
}

/// A request to open a Lightning channel from this node to Lexe's LSP.
#[derive(Serialize, Deserialize)]
pub struct OpenChannelRequest {
    /// The value of the channel to open.
    pub value: Amount,
    /// A user-provided id for this channel that's associated with the channel
    /// throughout its whole lifetime, as the Lightning protocol channel id is
    /// only known after negotiating the channel and creating the funding tx.
    ///
    /// This id is also used for idempotency. Retrying a request with the same
    /// `user_channel_id` won't accidentally open another channel.
    ///
    /// If `None`, a random id is generated, which provides no idempotency
    /// across separate `open_channel` calls.
    pub user_channel_id: Option<UserChannelId>,
}

/// The response to a request to open a channel to the LSP.
#[derive(Serialize, Deserialize)]
pub struct OpenChannelResponse {
    /// The id of the newly opened channel.
    pub channel_id: ChannelId,
    /// A user-provided id for this channel that's associated with the channel
    /// throughout its whole lifetime, as the Lightning protocol channel id is
    /// only known after negotiating the channel and creating the funding tx.
    pub user_channel_id: UserChannelId,
}

/// A request to close a Lightning channel between this node and Lexe's LSP.
#[derive(Serialize, Deserialize)]
pub struct CloseChannelRequest {
    /// The id of the channel to close.
    pub channel_id: ChannelId,
}

impl From<CloseChannelRequest> for command::CloseChannelRequest {
    fn from(req: CloseChannelRequest) -> Self {
        Self {
            channel_id: req.channel_id,
            // Always coop close; unilateral force close should be exposed
            // in the CLI
            force_close: false,
            // Let the node determine the counterparty via `list_channels`.
            maybe_counterparty: None,
        }
    }
}

// --- Client credentials management --- //

/// Information about a client that can authenticate with a Lexe node.
#[derive(Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize)]
pub struct ClientInfoResponse {
    /// Information about the client.
    pub client: ClientInfo,
}

/// The response to a request listing the active clients on a Lexe node.
#[derive(Serialize, Deserialize)]
pub struct ListClientsResponse {
    /// The clients that can authenticate with this node, mapped from
    /// client public key to client information.
    pub clients: HashMap<ed25519::PublicKey, ClientInfo>,
}

/// A request to create a new client and client credentials
/// that can authenticate with a Lexe node.
#[derive(Serialize, Deserialize)]
pub struct CreateClientRequest {
    /// An optional expiration for the client.
    ///
    /// [`None`] indicates that the client should never expire. Use carefully!
    pub expires_at: Option<TimestampMs>,
    /// An optional label for the client.
    ///
    /// Must be at most 64 UTF-8 bytes if provided.
    pub label: Option<String>,
    // TODO(nicole): Add scope when it's useful
    // pub scope: LexeScope,
}

// If this breaks, update the docs above.
const_assert_usize_eq!(revocable_clients::RevocableClient::MAX_LABEL_LEN, 64);

impl From<CreateClientRequest>
    for revocable_clients::models::CreateRevocableClientRequest
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
#[derive(Serialize, Deserialize)]
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
#[derive(Serialize, Deserialize)]
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

impl UpdateClientRequest {
    /// Build a request from explicit set/clear flags, used by wrappers whose
    /// serialization can't express the `Option<Option<_>>` fields, such as the
    /// Sidecar's JSON API, UniFFI, and the CLI.
    ///
    /// - At most one of `clear_label` or `label` can be set.
    /// - At most one of `clear_expiration` or `expires_at` can be set.
    pub fn new(
        client_pk: ed25519::PublicKey,
        label: Option<String>,
        clear_label: bool,
        expires_at: Option<TimestampMs>,
        clear_expiration: bool,
    ) -> anyhow::Result<Self> {
        ensure!(
            !(clear_label && label.is_some()),
            "Set only one of `label`, `clear_label`",
        );
        ensure!(
            !(clear_expiration && expires_at.is_some()),
            "Set only one of the expiration and `clear_expiration`",
        );

        // `Some(None)` clears the field; `None` leaves it unchanged.
        let new_label = if clear_label {
            Some(None)
        } else {
            label.map(Some)
        };
        let new_expires_at = if clear_expiration {
            Some(None)
        } else {
            expires_at.map(Some)
        };

        Ok(Self {
            client_pk,
            new_label,
            new_expires_at,
        })
    }
}

impl From<UpdateClientRequest>
    for revocable_clients::models::UpdateClientRequest
{
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
/// authentication. This cannot be undone.
#[derive(Serialize, Deserialize)]
pub struct RevokeClientRequest {
    /// The public key of the client to revoke.
    pub client_pk: ed25519::PublicKey,
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn update_client_req_from_flags() {
        let pk = ed25519::PublicKey::from_str(
            "b484a4890b47358ee68684bcd502d2eefa1bc66cc0f8ac2e5f06384676be74eb",
        )
        .unwrap();
        let ts = TimestampMs::from_millis(1_772_349_163_000).unwrap();
        let build = |label, clear, expires_at, never| {
            UpdateClientRequest::new(pk, label, clear, expires_at, never)
        };

        // Set: `Some(Some(_))`. Clear: `Some(None)`. Unchanged: `None`.
        let set = build(Some("hi".into()), false, Some(ts), false).unwrap();
        assert_eq!(set.new_label, Some(Some("hi".into())));
        assert_eq!(set.new_expires_at, Some(Some(ts)));

        let clear = build(None, true, None, true).unwrap();
        assert_eq!(clear.new_label, Some(None));
        assert_eq!(clear.new_expires_at, Some(None));

        let unchanged = build(None, false, None, false).unwrap();
        assert_eq!(unchanged.new_label, None);
        assert_eq!(unchanged.new_expires_at, None);

        // Conflicting set + clear flags are rejected.
        assert!(build(Some("hi".into()), true, None, false).is_err());
        assert!(build(None, false, Some(ts), true).is_err());
    }
}
