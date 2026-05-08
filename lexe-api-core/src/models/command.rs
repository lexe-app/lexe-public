use std::collections::BTreeSet;

use bitcoin::{address::NetworkUnchecked, bip32::Xpub};
#[cfg(doc)]
use lexe_common::root_seed::RootSeed;
#[cfg(any(test, feature = "test-utils"))]
use lexe_common::test_utils::arbitrary;
use lexe_common::{
    api::user::{NodePk, UserPk},
    ln::{
        amount::Amount,
        balance::{LightningBalance, OnchainBalance},
        channel::{LxChannelDetails, LxChannelId, LxUserChannelId},
        hashes::Txid,
        priority::ConfirmationPriority,
        route::LxRoute,
    },
    ppm::Ppm,
    time::TimestampMs,
};
use lexe_enclave::enclave::Measurement;
use lexe_serde::hexstr_or_bytes;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::types::{
    bounded_string::BoundedString,
    invoice::Invoice,
    offer::{MaxQuantity, Offer},
    payments::{
        ClientPaymentId, PaymentCreatedIndex, PaymentId, PaymentUpdatedIndex,
    },
    username::Username,
};

// --- General --- //

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfoV1 {
    pub version: semver::Version,
    pub measurement: Measurement,
    pub user_pk: UserPk,
    pub node_pk: NodePk,
    pub num_peers: usize,

    pub num_usable_channels: usize,
    pub num_channels: usize,
    /// Our lightning channel balance
    pub lightning_balance: LightningBalance,

    /// Our on-chain wallet balance
    pub onchain_balance: OnchainBalance,
    /// The total # of UTXOs tracked by BDK.
    pub num_utxos: usize,
    /// The # of confirmed UTXOs tracked by BDK.
    // TODO(max): LSP metrics should warn if this drops too low, as opening
    // zeroconf with unconfirmed inputs risks double spending of channel funds.
    pub num_confirmed_utxos: usize,
    /// The # of unconfirmed UTXOs tracked by BDK.
    pub num_unconfirmed_utxos: usize,

    /// The channel manager's best synced block height.
    pub best_block_height: u32,

    /// The number of pending channel monitor updates.
    /// If this isn't 0, it's likely that at least one channel is paused.
    // TODO(max): This field is in the wrong place and should be removed.
    // To my knowledge it is only used by integration tests (in a hacky way) to
    // wait for a node to reach a quiescent state. The polling should be done
    // inside the server handler rather than by the client in the test harness.
    pub pending_monitor_updates: usize,
}

/// Information about the Lexe node.
//
// This is a cleaned-up version of [`NodeInfoV1`] with diagnostic fields
// moved to [`DebugInfo`].
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub version: semver::Version,
    pub measurement: Measurement,
    pub user_pk: UserPk,
    pub node_pk: NodePk,
    pub num_peers: usize,

    pub num_usable_channels: usize,
    pub num_channels: usize,
    /// Our lightning channel balance.
    pub lightning_balance: LightningBalance,

    /// Our on-chain wallet balance.
    pub onchain_balance: OnchainBalance,

    /// The channel manager's best synced block height.
    pub best_block_height: u32,
}

impl From<NodeInfoV1> for NodeInfo {
    fn from(v1: NodeInfoV1) -> Self {
        Self {
            version: v1.version,
            measurement: v1.measurement,
            user_pk: v1.user_pk,
            node_pk: v1.node_pk,
            num_peers: v1.num_peers,
            num_usable_channels: v1.num_usable_channels,
            num_channels: v1.num_channels,
            lightning_balance: v1.lightning_balance,
            onchain_balance: v1.onchain_balance,
            best_block_height: v1.best_block_height,
        }
    }
}

/// Diagnostic information for debugging purposes.
#[derive(Debug, Serialize, Deserialize)]
pub struct DebugInfo {
    /// Output descriptors for the on-chain wallet.
    pub descriptors: OnchainDescriptors,
    /// Legacy descriptors for wallets created <= node-v0.9.2.
    /// `None` if the node doesn't have a legacy wallet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_descriptors: Option<OnchainDescriptors>,

    /// The total # of UTXOs tracked by BDK.
    pub num_utxos: usize,
    /// The # of confirmed UTXOs tracked by BDK.
    // TODO(max): LSP metrics should warn if this drops too low, as opening
    // zeroconf with unconfirmed inputs risks double spending of channel funds.
    pub num_confirmed_utxos: usize,
    /// The # of unconfirmed UTXOs tracked by BDK.
    pub num_unconfirmed_utxos: usize,

    /// The number of pending channel monitor updates.
    /// If this isn't 0, it's likely that at least one channel is paused.
    // TODO(max): This field is in the wrong place and should be removed.
    // To my knowledge it is only used by integration tests (in a hacky way) to
    // wait for a node to reach a quiescent state. The polling should be done
    // inside the server handler rather than by the client in the test harness.
    // This field is `Option` precisely so we can easily remove it later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_monitor_updates: Option<usize>,
}

/// BIP84 wpkh output descriptors for the on-chain wallet.
///
/// Descriptor strings include origin info and checksum. Example:
/// "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/0/*)#dwvchw0k"
///
/// These are copy-pasteable into other wallets like Sparrow.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainDescriptors {
    /// BIP389 multipath descriptor for both keychains:
    /// `wpkh([fp/84'/0'/0']xpub.../<0;1>/*)#checksum`
    pub multipath_descriptor: String,

    /// External (receive) keychain descriptor.
    pub external_descriptor: String,

    /// Internal (change) keychain descriptor.
    pub internal_descriptor: String,

    /// Account-level xpub at `m/84'/{coin}'/0'` for legacy tools.
    pub account_xpub: Xpub,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GDriveStatus {
    Ok,
    Error(String),
    Disabled,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupInfo {
    pub gdrive_status: GDriveStatus,
}

/// Request to query which node enclaves need provisioning, given the client's
/// trusted measurements.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct EnclavesToProvisionRequest {
    /// The enclave measurements the client trusts.
    /// Typically the 3 latest from releases.json.
    pub trusted_measurements: BTreeSet<Measurement>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct SetupGDrive {
    /// The auth `code` which can used to obtain a set of GDrive credentials.
    /// - Applicable only in staging/prod.
    /// - If GDrive has not been setup, the node will acquire the full set of
    ///   GDrive credentials and persist them (encrypted ofc) in Lexe's DB.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub google_auth_code: String,

    /// The password-encrypted [`RootSeed`] which can be backed up in
    /// GDrive.
    /// - Applicable only in staging/prod.
    /// - If Drive backup is not setup, instance will back up this encrypted
    ///   [`RootSeed`] in Google Drive. If a backup already exists, it is
    ///   overwritten.
    /// - We require the client to password-encrypt prior to sending the
    ///   provision request to prevent leaking the length of the password. It
    ///   also shifts the burden of running the 600K HMAC iterations from the
    ///   provision instance to the mobile app.
    #[serde(with = "hexstr_or_bytes")]
    pub encrypted_seed: Vec<u8>,
}
// --- Channel Management --- //

#[derive(Serialize, Deserialize)]
pub struct ListChannelsResponse {
    pub channels: Vec<LxChannelDetails>,
}

/// The information required for the user node to open a channel to the LSP.
#[derive(Serialize, Deserialize)]
pub struct OpenChannelRequest {
    /// A user-provided id for this channel that's associated with the channel
    /// throughout its whole lifetime, as the Lightning protocol channel id is
    /// only known after negotiating the channel and creating the funding tx.
    ///
    /// This id is also used for idempotency. Retrying a request with the same
    /// `user_channel_id` won't accidentally open another channel.
    pub user_channel_id: LxUserChannelId,
    /// The value of the channel we want to open.
    pub value: Amount,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenChannelResponse {
    /// The Lightning protocol channel id of the newly created channel.
    pub channel_id: LxChannelId,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightOpenChannelRequest {
    /// The value of the channel we want to open.
    pub value: Amount,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PreflightOpenChannelResponse {
    /// The estimated on-chain fee required to execute the channel open.
    pub fee_estimate: Amount,
}

#[derive(Serialize, Deserialize)]
pub struct CloseChannelRequest {
    /// The id of the channel we want to close.
    pub channel_id: LxChannelId,
    /// Set to true if the channel should be force closed (unilateral).
    /// Set to false if the channel should be cooperatively closed (bilateral).
    pub force_close: bool,
    /// The [`NodePk`] of our counterparty.
    ///
    /// If set to [`None`], the counterparty's [`NodePk`] will be determined by
    /// calling [`list_channels`]. Setting this to [`Some`] allows
    /// `close_channel` to avoid this relatively expensive [`Vec`] allocation.
    ///
    /// [`list_channels`]: lightning::ln::channelmanager::ChannelManager::list_channels
    pub maybe_counterparty: Option<NodePk>,
}

pub type PreflightCloseChannelRequest = CloseChannelRequest;

#[derive(Serialize, Deserialize)]
pub struct PreflightCloseChannelResponse {
    /// The estimated on-chain fee required to execute the channel close.
    pub fee_estimate: Amount,
}

// --- Syncing and updating payments data --- //

/// Upgradeable API struct for a [`PaymentId`].
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentIdStruct {
    /// The id of the payment to be fetched.
    pub id: PaymentId,
}

/// An upgradeable version of [`Vec<PaymentId>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct VecPaymentId {
    pub ids: Vec<PaymentId>,
}

/// Upgradeable API struct for a payment index.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentCreatedIndexStruct {
    /// The index of the payment to be fetched.
    pub index: PaymentCreatedIndex,
}

/// Sync a batch of new payments to local storage.
/// Results are returned in ascending `(created_at, payment_id)` order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNewPayments {
    /// Optional [`PaymentCreatedIndex`] at which the results should start,
    /// exclusive. Payments with an index less than or equal to this will
    /// not be returned.
    pub start_index: Option<PaymentCreatedIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Get a batch of payments in ascending `(updated_at, payment_id)` order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetUpdatedPayments {
    /// `(updated_at, id)` index at which the results should start, exclusive.
    /// Payments with an index less than or equal to this will not be returned.
    pub start_index: Option<PaymentUpdatedIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Get a batch of payment metadata in asc `(updated_at, payment_id)` order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetUpdatedPaymentMetadata {
    /// `(updated_at, id)` index at which the results should start, exclusive.
    /// Metadata with an index less than or equal to this will not be returned.
    pub start_index: Option<PaymentUpdatedIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Upgradeable API struct for a list of [`PaymentCreatedIndex`]s.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentCreatedIndexes {
    /// The string-serialized [`PaymentCreatedIndex`]s of the payments to be
    /// fetched. Typically, the ids passed here correspond to payments that
    /// the mobile client currently has stored locally as "pending"; the
    /// goal is to check whether any of these payments have been updated.
    pub indexes: Vec<PaymentCreatedIndex>,
}

/// A request to update the personal note on a payment. Pass `None` to clear.
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdatePersonalNote {
    /// The index of the payment whose personal note should be updated.
    // TODO(max): The server side only needs the `PaymentId`.
    // This API should be changed to pass that instead.
    pub index: PaymentCreatedIndex,
    /// The updated note, or `None` to clear.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "note", alias = "personal_note")]
    pub personal_note: Option<BoundedString>,
}

// --- BOLT11 Invoice Payments --- //

#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,

    /// The amount to encode into the invoice.
    pub amount: Option<Amount>,

    /// The description to be encoded into the invoice.
    ///
    /// If `None`, the `description` field inside the invoice will be an empty
    /// string (""), as lightning _requires_ a description (or description
    /// hash) to be set.
    /// NOTE: If both `description` and `description_hash` are set, node will
    /// return an error.
    pub description: Option<String>,

    /// A 256-bit hash. Commonly a hash of a long description.
    ///
    /// This field is used to associate description longer than 639 bytes to
    /// the invoice. Also known as '`h` tag in BOLT11'.
    ///
    /// This field is required to build invoices for the LNURL (LUD06)
    /// receiving flow. Not used in other flows.
    /// NOTE: If both `description` and `description_hash` are set, node will
    /// return an error.
    pub description_hash: Option<[u8; 32]>,

    /// An optional message from the payer, stored with this inbound payment.
    /// For LNURL-pay, set from the LUD-12 `comment`.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "payer_note", alias = "message")]
    pub message: Option<BoundedString>,

    /// The partner's user_pk, if the partner is setting the fee for this
    /// payment instead of using Lexe's default fees.
    ///
    /// This must be set in order for `partner_prop_fee` and `partner_base_fee`
    /// to take effect.
    // Added in `node-v0.9.6`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_pk: Option<UserPk>,

    /// The partner-chosen proportional fee to charge on this payment.
    /// If `partner_pk` is set, this must be set to [`Some`].
    ///
    /// Minimum: 5000 ppm (`LSP_USERNODE_SKIM_FEE`)
    /// Maximum: 500,000 ppm (50%)
    // Added in `node-v0.9.6`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_prop_fee: Option<Ppm>,

    /// The partner-chosen base fee to charge on this payment.
    ///
    /// If this is set, the invoice `amount` must also be set.
    // Added in `node-v0.9.6`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_base_fee: Option<Amount>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateInvoiceResponse {
    pub invoice: Invoice,
    /// The [`PaymentCreatedIndex`] of the newly created invoice payment.
    ///
    /// Is always `Some` starting at `node-v0.8.10` and `lsp-v0.8.11`.
    //
    // TODO(max): Make non-Option once all servers are sufficiently upgraded.
    pub created_index: Option<PaymentCreatedIndex>,
}

#[derive(Serialize, Deserialize)]
pub struct PayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: Invoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be [`Some`] for amountless invoices.
    pub fallback_amount: Option<Amount>,
    /// An optional message to persist with this outbound payment. For
    /// LNURL-pay, this is the LUD-12 `comment` sent during invoice
    /// negotiation.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "payer_note", alias = "message")]
    pub message: Option<BoundedString>,
    /// An optional personal note for this payment, useful if the
    /// receiver-provided description is insufficient.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "note", alias = "personal_note")]
    pub personal_note: Option<BoundedString>,
}

#[derive(Serialize, Deserialize)]
pub struct PayInvoiceResponse {
    /// When the node registered this payment.
    /// Used in the [`PaymentCreatedIndex`].
    pub created_at: TimestampMs,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: Invoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be [`Some`] for amountless invoices.
    pub fallback_amount: Option<Amount>,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayInvoiceResponse {
    /// The total amount to-be-paid for the pre-flighted [`Invoice`],
    /// excluding the fees.
    ///
    /// This value may be different from the value originally requested if
    /// we had to reach `htlc_minimum_msat` for some intermediate hops.
    pub amount: Amount,
    /// The total amount of fees to-be-paid for the pre-flighted [`Invoice`].
    pub fees: Amount,
    /// The route this invoice will be paid over.
    // Added in node,lsp-v0.7.8
    // TODO(max): We don't actually pay over this route.
    pub route: LxRoute,
}

// --- BOLT12 Offer payments --- //

#[derive(Default, Serialize, Deserialize)]
pub struct CreateOfferRequest {
    /// The description to be encoded into the invoice.
    ///
    /// If `None`, the `description` field inside the invoice will be an empty
    /// string (""), as lightning _requires_ a description to be set.
    pub description: Option<BoundedString>,

    /// The `min_amount` we're requesting for payments using this offer.
    ///
    /// If `None`, the offer is variable amount and the payer can choose any
    /// value.
    ///
    /// If `Some`, the offer amount is lower-bounded and the payer must pay
    /// this value or higher (per item, see `max_quantity`). The offer amount
    /// must be a non-zero value if set.
    // Renamed in node-v0.9.4
    #[serde(alias = "amount")]
    pub min_amount: Option<Amount>,

    /// An optional expiration for the offer, in seconds from now.
    pub expiry_secs: Option<u32>,

    /// The max number of items that can be purchased in any one payment for
    /// the offer.
    ///
    /// NOTE: this is not related to single-use vs reusable offers.
    ///                                                                        
    /// The expected amount paid for this offer is `offer.min_amount *
    /// quantity`, where `offer.min_amount` is the value per item and
    /// `quantity` is the number of items chosen _by the payer_. The
    /// payer's chosen `quantity` must be in the range: `0 < quantity <=
    /// offer.max_quantity`.
    ///
    /// If `None`, defaults to `MaxQuantity::ONE`, i.e., the expected paid
    /// `amount` is just `offer.amount`.
    pub max_quantity: Option<MaxQuantity>,

    /// The issuer of the offer.
    ///
    /// If `Some`, offer will encode the string. Bolt12 spec expects this tring
    /// to be a domain or a `user@domain` address.
    /// If `None`, offer issuer will encode "lexe.app" as the issuer.
    pub issuer: Option<BoundedString>,
    //
    // TODO(phlip9): add a `single_use` field to the offer request? right now
    // all offers are reusable.
}

#[derive(Serialize, Deserialize)]
pub struct CreateOfferResponse {
    pub offer: Offer,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayOfferRequest {
    /// The user-provided idempotency id for this payment.
    pub cid: ClientPaymentId,
    /// The offer we want to pay.
    pub offer: Offer,
    /// Specifies the amount we will pay. If the offer specifies a minimum
    /// amount, `amount` should satisfy that minimum.
    // Renamed and made non-optional in node-v0.9.4
    // The old `fallback_amount = None` is technically valid and incompatible
    // but rare, due to offers not setting amounts often
    #[serde(alias = "fallback_amount")]
    pub amount: Amount,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayOfferResponse {
    /// The total amount to-be-paid for the pre-flighted [`Offer`],
    /// excluding the fees.
    ///
    /// This value may be different from the value originally requested if
    /// we had to reach `htlc_minimum_msat` for some intermediate hops.
    pub amount: Amount,
    /// The total amount of fees to-be-paid for the pre-flighted [`Offer`].
    ///
    /// Since we only approximate the route atm, we likely underestimate the
    /// actual fee.
    pub fees: Amount,
    /// The route this offer will be paid over.
    ///
    /// Because we don't yet fetch the actual BOLT 12 invoice during preflight,
    /// this route is only an approximation of the final route (we can only
    /// route to the last public node before the offer's blinded path begins).
    // Added in node,lsp-v0.7.8
    // TODO(max): We don't actually pay over this route.
    pub route: LxRoute,
}

#[derive(Serialize, Deserialize)]
pub struct PayOfferRequest {
    /// The user-provided idempotency id for this payment.
    pub cid: ClientPaymentId,
    /// The offer we want to pay.
    pub offer: Offer,
    /// Specifies the amount we will pay. If the offer specifies a minimum
    /// amount, `amount` should satisfy that minimum.
    // Renamed and made non-optional in node-v0.9.4
    // The old `fallback_amount = None` is technically valid and incompatible
    // but rare, due to offers not setting amounts often
    #[serde(alias = "fallback_amount")]
    pub amount: Amount,
    /// An optional BOLT 12 `payer_note` included with the invoice request and
    /// visible to the recipient.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "payer_note", alias = "message")]
    pub message: Option<BoundedString>,
    /// An optional personal note for this payment, useful if the
    /// receiver-provided description is insufficient.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "note", alias = "personal_note")]
    pub personal_note: Option<BoundedString>,
}

#[derive(Serialize, Deserialize)]
pub struct PayOfferResponse {
    /// When the node registered this payment. Used in the
    /// [`PaymentCreatedIndex`].
    pub created_at: TimestampMs,
}

// --- On-chain payments --- //

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetAddressResponse {
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_mainnet_addr_unchecked()")
    )]
    pub addr: bitcoin::Address<NetworkUnchecked>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary, Debug))]
pub struct PayOnchainRequest {
    /// The identifier to use for this payment.
    pub cid: ClientPaymentId,
    /// The address we want to send funds to.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_mainnet_addr_unchecked()")
    )]
    pub address: bitcoin::Address<NetworkUnchecked>,
    /// How much Bitcoin we want to send.
    pub amount: Amount,
    /// How quickly we want our transaction to be confirmed.
    /// The higher the priority, the more fees we will pay.
    // See LexeEsplora for the conversion to the target number of blocks
    pub priority: ConfirmationPriority,
    /// An optional personal note for this payment.
    // compat: Alias added in node-v0.9.7
    #[serde(rename = "note", alias = "personal_note")]
    pub personal_note: Option<BoundedString>,
}

#[derive(Serialize, Deserialize)]
pub struct PayOnchainResponse {
    /// When the node registered this payment. Used in the
    /// [`PaymentCreatedIndex`].
    pub created_at: TimestampMs,
    /// The Bitcoin txid for the transaction we just submitted to the mempool.
    pub txid: Txid,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PreflightPayOnchainRequest {
    /// The address we want to send funds to.
    pub address: bitcoin::Address<NetworkUnchecked>,
    /// How much Bitcoin we want to send.
    pub amount: Amount,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayOnchainResponse {
    /// Corresponds with [`ConfirmationPriority::High`]
    ///
    /// The high estimate is optional--we don't want to block the user from
    /// sending if they only have enough for a normal tx fee.
    pub high: Option<FeeEstimate>,
    /// Corresponds with [`ConfirmationPriority::Normal`]
    pub normal: FeeEstimate,
    /// Corresponds with [`ConfirmationPriority::Background`]
    pub background: FeeEstimate,
}

#[derive(Serialize, Deserialize)]
pub struct FeeEstimate {
    /// The fee amount estimate.
    pub amount: Amount,
}

// --- Sync --- //

#[derive(Serialize, Deserialize)]
pub struct ResyncRequest {
    /// If true, the LSP will full sync the BDK wallet and do a normal LDK
    /// sync.
    pub full_sync: bool,
}

// --- Username --- //

/// Creates or updates a human Bitcoin address.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UpdateHumanBitcoinAddress {
    /// Username for BIP-353 and LNURL.
    pub username: Username,
    /// Offer to be used to fetch invoices on BIP-353.
    pub offer: Offer,
}

/// Claims a generated human Bitcoin address.
///
/// This endpoint is used during node initialization to claim an auto-generated
/// human Bitcoin address. The address will have `is_primary: false` and
/// `is_generated: true`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ClaimGeneratedHumanBitcoinAddress {
    /// Offer to be used to fetch invoices on BIP-353.
    pub offer: Offer,
    /// The username to claim. This must be the username returned by
    /// `get_generated_username`.
    pub username: Username,
}

/// Response for `get_generated_username` endpoint.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetGeneratedUsernameResponse {
    /// The generated username that can be used for claiming an HBA.
    pub username: Username,
    /// Whether this user already has a claimed generated HBA.
    /// If true, the caller should skip calling
    /// `claim_generated_human_bitcoin_address`.
    pub already_claimed: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct HumanBitcoinAddress {
    /// Current username for BIP-353 and LNURL.
    pub username: Option<Username>,
    /// Current offer for fetching invoices on BIP-353.
    pub offer: Option<Offer>,
    /// Last time the human Bitcoin address was updated.
    pub updated_at: Option<TimestampMs>,
    /// Whether the human Bitcoin address can be updated. Always `true` for
    /// generated addresses; for claimed addresses, depends on time-based
    /// freeze rules.
    pub updatable: bool,
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{Arbitrary, any},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for PreflightPayOnchainRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (arbitrary::any_mainnet_addr_unchecked(), any::<Amount>())
                .prop_map(|(address, amount)| Self { address, amount })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use lexe_common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn preflight_pay_onchain_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<PreflightPayOnchainRequest>(
        );
    }

    #[test]
    fn payment_id_struct_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<PaymentIdStruct>();
    }

    #[test]
    fn payment_index_struct_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<PaymentCreatedIndexStruct>(
        );
    }

    #[test]
    fn get_new_payments_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<GetNewPayments>();
    }

    #[test]
    fn payment_indexes_roundtrip() {
        // This is serialized as JSON, not query strings.
        roundtrip::json_value_roundtrip_proptest::<PaymentCreatedIndexes>();
    }

    #[test]
    fn get_address_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<GetAddressResponse>();
    }

    #[test]
    fn setup_gdrive_request_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<SetupGDrive>();
    }

    #[test]
    fn human_bitcoin_address_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateHumanBitcoinAddress>();
    }

    #[test]
    fn claim_generated_human_bitcoin_address_request_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<
            ClaimGeneratedHumanBitcoinAddress,
        >();
    }

    #[test]
    fn get_generated_username_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<GetGeneratedUsernameResponse>(
        );
    }

    #[test]
    fn human_bitcoin_address_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<HumanBitcoinAddress>();
    }

    /// Sanity check the `DebugInfo` serialization against a hard-coded string.
    #[test]
    fn debug_info_serialization() {
        // Account-level xpub (from BDK tests)
        let account_xpub = Xpub::from_str(
            "xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA"
        ).unwrap();

        // Descriptors with real checksums (computed via miniscript)
        let descriptor = "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/<0;1>/*)#c8v4zjyh".to_owned();
        let external_descriptor = "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/0/*)#dwvchw0k".to_owned();
        let internal_descriptor = "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/1/*)#u6fe2mlw".to_owned();

        let debug_info = DebugInfo {
            descriptors: OnchainDescriptors {
                multipath_descriptor: descriptor.clone(),
                external_descriptor: external_descriptor.clone(),
                internal_descriptor: internal_descriptor.clone(),
                account_xpub,
            },
            legacy_descriptors: None,
            num_utxos: 5,
            num_confirmed_utxos: 3,
            num_unconfirmed_utxos: 2,
            pending_monitor_updates: Some(0),
        };

        // Serialize and check against expected JSON.
        // NOTE: Do NOT remove this raw string check. We're sanity-checking how
        // it looks in serialized form.
        let json = serde_json::to_string_pretty(&debug_info).unwrap();
        let expected = r#"{
  "descriptors": {
    "multipath_descriptor": "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/<0;1>/*)#c8v4zjyh",
    "external_descriptor": "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/0/*)#dwvchw0k",
    "internal_descriptor": "wpkh([be83839f/84'/0'/0']xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA/1/*)#u6fe2mlw",
    "account_xpub": "xpub6DCQ1YcqvZtSwGWMrwHELPehjWV3f2MGZ69yBADTxFEUAoLwb5Mp5GniQK6tTp3AgbngVz9zEFbBJUPVnkG7LFYt8QMTfbrNqs6FNEwAPKA"
  },
  "num_utxos": 5,
  "num_confirmed_utxos": 3,
  "num_unconfirmed_utxos": 2,
  "pending_monitor_updates": 0
}"#;
        assert_eq!(json, expected);

        // Verify deserialization roundtrips
        let back: DebugInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.num_utxos, 5);
        assert_eq!(back.num_confirmed_utxos, 3);
        assert_eq!(back.num_unconfirmed_utxos, 2);
        assert_eq!(back.descriptors.multipath_descriptor, descriptor);
        assert_eq!(back.descriptors.external_descriptor, external_descriptor);
        assert_eq!(back.descriptors.internal_descriptor, internal_descriptor);
        assert_eq!(
            back.descriptors.account_xpub,
            debug_info.descriptors.account_xpub
        );
        assert!(back.legacy_descriptors.is_none());
        assert_eq!(back.pending_monitor_updates, Some(0));
    }
}
