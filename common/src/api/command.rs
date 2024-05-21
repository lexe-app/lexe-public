use bitcoin::Address;
use serde::{Deserialize, Serialize};

use crate::{
    api::NodePk,
    enclave::Measurement,
    ln::{
        amount::Amount, balance::Balance, channel::ChannelId,
        invoice::LxInvoice, payments::ClientPaymentId, ConfirmationPriority,
    },
};

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub version: semver::Version,
    pub measurement: Measurement,
    pub node_pk: NodePk,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub lightning_balance: Amount,
    pub num_peers: usize,
    /// Our on-chain wallet [`Balance`].
    pub onchain_balance: Balance,
    /// The number of pending channel monitor updates.
    /// If this isn't 0, it's likely that at least one channel is paused.
    pub pending_monitor_updates: usize,
}

/// The information required for the user node to open a channel to the LSP.
#[derive(Serialize, Deserialize)]
pub struct OpenChannelRequest {
    /// The value of the channel we want to open.
    pub value: Amount,
}

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
    /// This value may be different from the value originally requested if some
    /// we had to reach `htlc_minimum_msat` for some intermediate hops.
    pub amount: Amount,
    /// The total amount of fees to-be-paid for the pre-flighted [`LxInvoice`].
    pub fees: Amount,
}

#[derive(Serialize, Deserialize)]
pub struct SendOnchainRequest {
    /// The identifier to use for this payment.
    pub cid: ClientPaymentId,
    /// The address we want to send funds to.
    pub address: Address,
    /// How much Bitcoin we want to send.
    pub amount: Amount,
    /// How quickly we want our transaction to be confirmed.
    /// The higher the priority, the more fees we will pay.
    // See LexeEsplora for the conversion to the target number of blocks
    pub priority: ConfirmationPriority,
    /// An optional personal note for this payment.
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct EstimateFeeSendOnchainRequest {
    /// The address we want to send funds to.
    pub address: Address,
    /// How much Bitcoin we want to send.
    pub amount: Amount,
}

#[derive(Serialize, Deserialize)]
pub struct EstimateFeeSendOnchainResponse {
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

#[derive(Serialize, Deserialize)]
pub struct CloseChannelRequest {
    /// The id of the channel we want to close.
    pub channel_id: ChannelId,
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

impl NodeInfo {
    /// The top-level, user-visible balance on e.g. the wallet home screen.
    /// This is the lightning balance (`local_balance`, always spendable) plus
    /// the on-chain balance (`confirmed_sat` + `trusted_pending_sat`,
    /// almost-certainly-spendable).
    pub fn spendable_balance(&self) -> Amount {
        Amount::try_from_sats_u64(self.onchain_balance.get_spendable())
            .ok()
            .and_then(|b| b.checked_add(self.lightning_balance))
            .expect("Overflow computing user's spendable wallet balance")
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::test_utils::arbitrary::any_mainnet_address;

    impl Arbitrary for EstimateFeeSendOnchainRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any_mainnet_address(), any::<Amount>())
                .prop_map(|(address, amount)| Self { address, amount })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip::query_string_roundtrip_proptest;

    #[test]
    fn estimate_fee_send_onchain_roundtrip() {
        query_string_roundtrip_proptest::<EstimateFeeSendOnchainRequest>();
    }
}
