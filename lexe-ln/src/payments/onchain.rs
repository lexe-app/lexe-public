use bitcoin::Txid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum OnchainPayment {
    Inbound(OnchainDeposit),
    Outbound(OnchainWithdrawal),
}

// --- Inbound onchain payments --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainDeposit {}

// --- Outbound on-chain payments --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainWithdrawal {}

impl OnchainPayment {
    /// Returns the [`Txid`] of this payment.
    pub fn txid(&self) -> &Txid {
        todo!()
    }
}
