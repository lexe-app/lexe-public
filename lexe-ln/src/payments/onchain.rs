use bitcoin::Txid;
use common::time::TimestampMillis;
use serde::{Deserialize, Serialize};

use crate::payments::{PaymentDirection, PaymentStatus, PaymentTrait};

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

impl PaymentTrait for OnchainPayment {
    fn direction(&self) -> PaymentDirection {
        match self {
            Self::Inbound(..) => PaymentDirection::Inbound,
            Self::Outbound(..) => PaymentDirection::Outbound,
        }
    }

    fn amt_msat(&self) -> Option<u64> {
        todo!()
    }

    fn fees_msat(&self) -> u64 {
        todo!()
    }

    fn status(&self) -> PaymentStatus {
        todo!()
    }

    fn status_str(&self) -> &str {
        todo!()
    }

    fn created_at(&self) -> TimestampMillis {
        todo!()
    }

    fn finalized_at(&self) -> Option<TimestampMillis> {
        todo!()
    }
}
