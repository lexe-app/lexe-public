use bitcoin::Address;
use serde::{Deserialize, Serialize};

use crate::{
    api::NodePk,
    ln::{
        amount::Amount, balance::Balance, invoice::LxInvoice,
        ConfirmationPriority,
    },
};

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub node_pk: NodePk,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance: Amount,
    pub num_peers: usize,
    /// Our on-chain wallet [`Balance`].
    pub wallet_balance: Balance,
}

#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,
    pub amount: Option<Amount>,
    /// The description to be encoded into the invoice.
    pub description: String,
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
pub struct SendOnchainRequest {
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
