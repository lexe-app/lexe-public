use serde::{Deserialize, Serialize};

use crate::payments::PaymentDirection;

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
    /// Whether this payment is inbound or outbound. Useful for filtering.
    pub fn direction(&self) -> PaymentDirection {
        match self {
            Self::Inbound(..) => PaymentDirection::Inbound,
            Self::Outbound(..) => PaymentDirection::Outbound,
        }
    }
}
