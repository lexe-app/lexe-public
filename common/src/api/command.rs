use bitcoin::address::NetworkUnchecked;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    api::user::NodePk,
    enclave::Measurement,
    ln::{
        amount::Amount,
        balance::{LightningBalance, OnchainBalance},
        channel::{LxChannelDetails, LxChannelId, LxUserChannelId},
        hashes::LxTxid,
        invoice::LxInvoice,
        offer::{LxOffer, MaxQuantity},
        payments::{ClientPaymentId, PaymentIndex},
        priority::ConfirmationPriority,
        route::LxRoute,
    },
    time::TimestampMs,
};

// --- General --- //

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub version: semver::Version,
    pub measurement: Measurement,
    pub node_pk: NodePk,
    pub num_peers: usize,
    pub num_usable_channels: usize,
    pub num_channels: usize,
    /// Our lightning channel balance
    pub lightning_balance: LightningBalance,
    /// Our on-chain wallet balance
    pub onchain_balance: OnchainBalance,
    /// The number of pending channel monitor updates.
    /// If this isn't 0, it's likely that at least one channel is paused.
    pub pending_monitor_updates: usize,
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

/// Upgradeable API struct for a payment index.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentIndexStruct {
    /// The index of the payment to be fetched.
    // We use index instead of id so the backend can query by primary key.
    pub index: PaymentIndex,
}

/// Sync a batch of new payments to local storage.
/// Results are returned in ascending `(created_at, payment_id)` order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNewPayments {
    /// Optional [`PaymentIndex`] at which the results should start, exclusive.
    /// Payments with an index less than or equal to this will not be returned.
    pub start_index: Option<PaymentIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Upgradeable API struct for a list of [`PaymentIndex`]s.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentIndexes {
    /// The string-serialized [`PaymentIndex`]s of the payments to be fetched.
    /// Typically, the ids passed here correspond to payments that the mobile
    /// client currently has stored locally as "pending"; the intention is to
    /// check whether any of these payments have been updated.
    pub indexes: Vec<PaymentIndex>,
}

/// Update the note on a payment.
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdatePaymentNote {
    /// The index of the payment whose note should be updated.
    pub index: PaymentIndex,
    /// The updated note.
    pub note: Option<String>,
}

// --- BOLT11 Invoice Payments --- //

#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,
    pub amount: Option<Amount>,
    /// The description to be encoded into the invoice.
    ///
    /// If `None`, the `description` field inside the invoice will be an empty
    /// string (""), as lightning _requires_ a description (or description
    /// hash) to be set.
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateInvoiceResponse {
    pub invoice: LxInvoice,
}

#[derive(Serialize, Deserialize)]
pub struct PayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: LxInvoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be [`Some`] for amountless invoices.
    pub fallback_amount: Option<Amount>,
    /// An optional personal note for this payment, useful if the
    /// receiver-provided description is insufficient.
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PayInvoiceResponse {
    /// When the node registered this payment. Used in the [`PaymentIndex`].
    ///
    /// [`PaymentIndex`]: crate::ln::payments::PaymentIndex
    pub created_at: TimestampMs,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: LxInvoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be [`Some`] for amountless invoices.
    pub fallback_amount: Option<Amount>,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayInvoiceResponse {
    /// The total amount to-be-paid for the pre-flighted [`LxInvoice`],
    /// excluding the fees.
    ///
    /// This value may be different from the value originally requested if
    /// we had to reach `htlc_minimum_msat` for some intermediate hops.
    pub amount: Amount,
    /// The total amount of fees to-be-paid for the pre-flighted [`LxInvoice`].
    pub fees: Amount,
    /// The route this invoice will be paid over.
    // Added in node,lsp-v0.7.8
    // TODO(max): We don't actually pay over this route.
    pub route: LxRoute,
}

// --- BOLT12 Offer payments --- //

#[derive(Serialize, Deserialize)]
pub struct CreateOfferRequest {
    pub expiry_secs: Option<u32>,
    /// The `amount` we're requesting for payments using this offer.
    ///
    /// If `None`, the offer is variable amount and the payer can choose any
    /// value.
    ///
    /// If `Some`, the offer amount is fixed and the payer must pay exactly
    /// this value (per item, see `max_quantity`).
    pub amount: Option<Amount>,
    /// The description to be encoded into the invoice.
    ///
    /// If `None`, the `description` field inside the invoice will be an empty
    /// string (""), as lightning _requires_ a description to be set.
    pub description: Option<String>,
    /// The max number of items that can be purchased in any one payment for
    /// the offer.
    ///
    /// NOTE: this is not related to single-use vs reusable offers.
    ///                                                                        
    /// The expected amount paid for this offer is `offer.amount * quantity`,
    /// where `offer.amount` is the value per item and `quantity` is the number
    /// of items chosen _by the payer_. The payer's chosen `quantity` must be
    /// in the range: `0 < quantity <= offer.max_quantity`.
    ///
    /// If `None`, defaults to `MaxQuantity::ONE`, i.e., the expected paid
    /// `amount` is just `offer.amount`.
    pub max_quantity: Option<MaxQuantity>,
    //
    // TODO(phlip9): add a `single_use` field to the offer request? right now
    // all offers are reusable.
}

#[derive(Serialize, Deserialize)]
pub struct CreateOfferResponse {
    pub offer: LxOffer,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayOfferRequest {
    /// The user-provided idempotency id for this payment.
    pub cid: ClientPaymentId,
    /// The offer we want to pay.
    pub offer: LxOffer,
    /// Specifies the amount we will pay if the offer to be paid is
    /// amountless. This field must be [`Some`] for amountless offers.
    pub fallback_amount: Option<Amount>,
}

#[derive(Serialize, Deserialize)]
pub struct PreflightPayOfferResponse {
    /// The total amount to-be-paid for the pre-flighted [`LxOffer`],
    /// excluding the fees.
    ///
    /// This value may be different from the value originally requested if
    /// we had to reach `htlc_minimum_msat` for some intermediate hops.
    pub amount: Amount,
    /// The total amount of fees to-be-paid for the pre-flighted [`LxOffer`].
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
    pub offer: LxOffer,
    /// Specifies the amount we will pay if the offer to be paid is
    /// amountless. This field must be [`Some`] for amountless offers.
    pub fallback_amount: Option<Amount>,
    /// An optional personal note for this payment, useful if the
    /// receiver-provided description is insufficient.
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PayOfferResponse {
    /// When the node registered this payment. Used in the [`PaymentIndex`].
    ///
    /// [`PaymentIndex`]: crate::ln::payments::PaymentIndex
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
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct PayOnchainResponse {
    /// When the node registered this payment. Used in the [`PaymentIndex`].
    ///
    /// [`PaymentIndex`]: crate::ln::payments::PaymentIndex
    pub created_at: TimestampMs,
    /// The Bitcoin txid for the transaction we just submitted to the mempool.
    pub txid: LxTxid,
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

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{any, Arbitrary},
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
    use super::*;
    use crate::test_utils::roundtrip::{self, query_string_roundtrip_proptest};

    #[test]
    fn preflight_pay_onchain_roundtrip() {
        query_string_roundtrip_proptest::<PreflightPayOnchainRequest>();
    }

    #[test]
    fn payment_index_struct_roundtrip() {
        query_string_roundtrip_proptest::<PaymentIndexStruct>();
    }

    #[test]
    fn get_new_payments_roundtrip() {
        query_string_roundtrip_proptest::<GetNewPayments>();
    }

    #[test]
    fn payment_indexes_roundtrip() {
        // This is serialized as JSON, not query strings.
        roundtrip::json_value_roundtrip_proptest::<PaymentIndexes>();
    }

    #[test]
    fn get_address_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<GetAddressResponse>();
    }
}
