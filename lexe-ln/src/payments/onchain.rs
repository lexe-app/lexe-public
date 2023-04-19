use common::ln::amount::Amount;
use common::ln::hashes::LxTxid;
#[cfg(test)]
use common::test_utils::arbitrary;
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

// --- Onchain send --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainSend {
    pub txid: LxTxid,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainPaymentStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment. The user can only add a
    /// note after this onchain receive has been detected.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

// --- Onchain receive --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainReceive {
    pub txid: LxTxid,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainPaymentStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment.
    /// The user has the option to set this at payment creation time.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}
