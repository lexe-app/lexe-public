use serde::{Deserialize, Serialize};

use crate::api::NodePk;
use crate::ln::amount::Amount;
use crate::ln::channel::LxChannelDetails;
use crate::ln::invoice::LxInvoice;

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub node_pk: NodePk,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance: Amount,
    pub num_peers: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ListChannels {
    pub channel_details: Vec<LxChannelDetails>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,
    pub amount: Option<Amount>,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct PayInvoiceRequest {
    /// The invoice we want to pay.
    pub invoice: LxInvoice,
    /// Specifies the amount we will pay if the invoice to be paid is
    /// amountless. This field must be [`Some`] for amountless invoices.
    pub fallback_amount: Option<Amount>,
}
