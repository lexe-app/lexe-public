use anyhow::{bail, ensure};
use bitcoin::Transaction;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    api::command::SendOnchainRequest,
    ln::{amount::Amount, hashes::LxTxid, ConfirmationPriority},
    time::TimestampMs,
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::warn;

// --- Onchain send --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainSend {
    pub txid: LxTxid,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_raw_tx()"))]
    pub tx: Transaction,
    pub priority: ConfirmationPriority,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainSendStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment. The user can only add a
    /// note after this onchain receive has been detected.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OnchainSendStatus {
    AwaitingBroadcast,
    Confirming,
    Completed,
    Replaced,
    Reorged,
}

impl OnchainSend {
    pub fn new(tx: Transaction, req: SendOnchainRequest, fees: Amount) -> Self {
        Self {
            txid: LxTxid(tx.txid()),
            tx,
            priority: req.priority,
            amount: req.amount,
            fees,
            status: OnchainSendStatus::AwaitingBroadcast,
            created_at: TimestampMs::now(),
            note: req.note,
            finalized_at: None,
        }
    }

    pub fn broadcasted(
        &self,
        broadcasted_txid: &LxTxid,
    ) -> anyhow::Result<Self> {
        use OnchainSendStatus::*;

        ensure!(broadcasted_txid == &self.txid, "Txids don't match");

        match self.status {
            AwaitingBroadcast => (),
            Confirming => warn!("Transaction was already broadcasted"),
            Completed | Replaced | Reorged => bail!("Tx already finalized"),
        }

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = Confirming;

        Ok(clone)
    }
}

// --- Onchain receive --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainReceive {
    pub txid: LxTxid,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainReceiveStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment.
    /// The user has the option to set this at payment creation time.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OnchainReceiveStatus {
    Confirming,
    Completed,
    Replaced,
    Reorged,
}
