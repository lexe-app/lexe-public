use common::ln::amount::Amount;
use common::ln::hashes::LxTxid;
use common::time::TimestampMs;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
// TODO(max): Revisit these states once we actually implement onchain payments
pub enum OnchainPaymentStatus {
    Confirming,
    Completed,
    Replaced,
    Reorged,
}

// --- Onchain deposits --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainDeposit {
    pub txid: LxTxid,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainPaymentStatus,
    pub created_at: TimestampMs,
    pub finalized_at: Option<TimestampMs>,
}

// --- Onchain withdrawals --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainWithdrawal {
    pub txid: LxTxid,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainPaymentStatus,
    pub created_at: TimestampMs,
    pub finalized_at: Option<TimestampMs>,
}
