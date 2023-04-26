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

use crate::esplora::{TxConfQueryInfo, TxConfStatus};

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
    /// An optional personal note for this payment.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OnchainSendStatus {
    /// (Pending, not broadcasted) The tx has been created and signed but
    /// hasn't been broadcasted yet.
    Created,

    /// (Pending, zeroconf) The tx has been broadcasted and is awaiting its
    /// first confirmation.
    Broadcasted,
    /// (Pending, zeroconf) We broadcasted a RBF replacement or some other
    /// transaction that spends at least one of this transaction's inputs, with
    /// the intention of confirming the newer transaction.
    ReplacementBroadcasted,

    /// (Pending, 1-5 confs) The tx has at least 1 conf, but no greater than 5.
    PartiallyConfirmed,
    /// (Pending, 1-5 confs) At least one of this tx's inputs has been included
    /// in a different tx which has at least 1 conf, but no greater than 5.
    PartiallyReplaced,

    /// (Finalized-Completed, 6+ confs) The tx has 6 or more confirmations.
    FullyConfirmed,
    /// (Finalized-Failed, 6+ confs) At least one of this tx's inputs has been
    /// spent by a different tx which has between 6 or greater confirmations.
    FullyReplaced,
    /// (Finalized-Failed, zeroconf) All of the following are true:
    ///
    /// - This tx has not received a single confirmation.
    /// - We have not detected a replacement tx spending at least one of this
    ///   tx's inputs with 1 or more confirmations.
    /// - It has been at least 14 days since we first created this transaction.
    ///
    /// 14 days is the default `-mempoolexpiry` value in Bitcoin Core. It is
    /// likely that most nodes will have evicted our transaction from their
    /// mempool by now. There is a small chance that this transaction ends up
    /// getting confirmed, but we'll mark it as failed in our payments manager
    /// and move on, since this isn't security-critical; the user will still
    /// see the successful send reflected in their wallet balance.
    Dropped,
}

impl OnchainSend {
    pub fn new(tx: Transaction, req: SendOnchainRequest, fees: Amount) -> Self {
        Self {
            txid: LxTxid(tx.txid()),
            tx,
            priority: req.priority,
            amount: req.amount,
            fees,
            status: OnchainSendStatus::Created,
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
            Created => (),
            Broadcasted => warn!("We broadcasted this transaction twice"),
            PartiallyConfirmed => bail!("Tx already has confirmations"),
            ReplacementBroadcasted => bail!("Tx was being replaced"),
            PartiallyReplaced => bail!("Tx already partially replaced"),
            FullyConfirmed | FullyReplaced | Dropped => bail!("Tx was final"),
        }

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = Broadcasted;

        Ok(clone)
    }

    pub(crate) fn check_onchain_conf(
        &self,
        conf_status: TxConfStatus,
    ) -> anyhow::Result<Option<Self>> {
        use OnchainSendStatus::*;

        // We'll update our state if and only if (1) the payment is still in a
        // pending state and (2) the tx has been broadcasted.
        match self.status {
            Created => {
                warn!("Skipping conf status update; waiting for broadcast");
                return Ok(None);
            }
            Broadcasted
            | PartiallyConfirmed
            | ReplacementBroadcasted
            | PartiallyReplaced => (),
            FullyConfirmed | FullyReplaced | Dropped => bail!(
                "Tx already finalized; shouldn't have checked for conf status"
            ),
        }

        let updated_status = match conf_status {
            // If zeroconf, retain the current (Pending, zeroconf) state;
            // otherwise, revert to the broadcasted state. It is possible that
            // the `ReplacementBroadcasted` state gets lost due to getting a
            // confirmation from a block that is later reorged, but this should
            // be rare and it doesn't matter; all it affects is UI code.
            TxConfStatus::ZeroConf => match self.status {
                Broadcasted => Broadcasted,
                ReplacementBroadcasted => ReplacementBroadcasted,
                _ => Broadcasted,
            },
            TxConfStatus::InBestChain { confs } =>
                if confs < 6 {
                    PartiallyConfirmed
                } else {
                    FullyConfirmed
                },
            TxConfStatus::HasReplacement { confs } =>
                if confs < 6 {
                    PartiallyReplaced
                } else {
                    FullyReplaced
                },
            TxConfStatus::Dropped => Dropped,
        };

        // To prevent redundantly repersisting the same data, return Some(..)
        // only if the state has actually changed.
        if self.status == updated_status {
            Ok(None)
        } else {
            let mut clone = self.clone();
            clone.status = updated_status;
            if matches!(
                updated_status,
                FullyConfirmed | FullyReplaced | Dropped
            ) {
                clone.finalized_at = Some(TimestampMs::now());
            }

            Ok(Some(clone))
        }
    }

    pub fn to_query_info(&self) -> TxConfQueryInfo {
        TxConfQueryInfo {
            txid: self.txid,
            inputs: self
                .tx
                .input
                .iter()
                .map(|txin| txin.previous_output)
                .collect(),
            created_at: self.created_at.into(),
        }
    }
}

// --- Onchain receive --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainReceive {
    pub txid: LxTxid,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_raw_tx()"))]
    pub tx: Transaction,
    pub amount: Amount,
    pub status: OnchainReceiveStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment. Is set to [`None`] when the
    /// payment is first detected, but the user can add or modify it later.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OnchainReceiveStatus {
    /// (Pending, zeroconf) We detected the inbound tx, but it is still
    /// awaiting its first confirmation.
    Zeroconf,

    /// (Pending, 1-5 confs) The tx has at least 1 conf, but no greater than 5.
    PartiallyConfirmed,
    /// (Pending, 1-5 confs) At least one of this tx's inputs has been included
    /// in a different tx which has at least 1 conf, but no greater than 5.
    PartiallyReplaced,

    /// (Finalized-Completed, 6+ confs) The tx has 6 or more confirmations.
    FullyConfirmed,
    /// (Finalized-Failed, 6+ confs) At least one of this tx's inputs has been
    /// spent by a different tx which has between 6 or greater confirmations.
    FullyReplaced,
    /// (Finalized-Failed, zeroconf) All of the following are true:
    ///
    /// - This tx has not received a single confirmation.
    /// - We have not detected a replacement tx spending at least one of this
    ///   tx's inputs with 1 or more confirmations.
    /// - It has been at least 14 days since we first detected this
    ///   transaction.
    ///
    /// 14 days is the default `-mempoolexpiry` value in Bitcoin Core. It is
    /// likely that most nodes will have evicted our transaction from their
    /// mempool by now. There is a small chance that this transaction ends up
    /// getting confirmed, but we'll mark it as failed in our payments manager
    /// and move on, since this isn't security-critical; the user will still
    /// see the successful receive reflected in their wallet balance.
    Dropped,
}

impl OnchainReceive {
    pub(crate) fn new(tx: Transaction, amount: Amount) -> Self {
        Self {
            txid: LxTxid(tx.txid()),
            tx,
            amount,
            // Start at zeroconf and let the checker update it later.
            status: OnchainReceiveStatus::Zeroconf,
            created_at: TimestampMs::now(),
            note: None,
            finalized_at: None,
        }
    }

    pub(crate) fn check_onchain_conf(
        &self,
        conf_status: TxConfStatus,
    ) -> anyhow::Result<Option<Self>> {
        use OnchainReceiveStatus::*;

        // We'll update our state if and only if the payment is still pending.
        match self.status {
            Zeroconf | PartiallyConfirmed | PartiallyReplaced => (),
            FullyConfirmed | FullyReplaced | Dropped => bail!(
                "Tx already finalized; shouldn't have checked for conf status"
            ),
        }

        let updated_status = match conf_status {
            TxConfStatus::ZeroConf => Zeroconf,
            TxConfStatus::InBestChain { confs } =>
                if confs < 6 {
                    PartiallyConfirmed
                } else {
                    FullyConfirmed
                },
            TxConfStatus::HasReplacement { confs } =>
                if confs < 6 {
                    PartiallyReplaced
                } else {
                    FullyReplaced
                },
            TxConfStatus::Dropped => Dropped,
        };

        // To prevent redundantly repersisting the same data, return Some(..)
        // only if the state has actually changed.
        if self.status == updated_status {
            Ok(None)
        } else {
            let mut clone = self.clone();
            clone.status = updated_status;
            if matches!(
                updated_status,
                FullyConfirmed | FullyReplaced | Dropped
            ) {
                clone.finalized_at = Some(TimestampMs::now());
            }

            Ok(Some(clone))
        }
    }

    pub fn to_query_info(&self) -> TxConfQueryInfo {
        TxConfQueryInfo {
            txid: self.txid,
            inputs: self
                .tx
                .input
                .iter()
                .map(|txin| txin.previous_output)
                .collect(),
            created_at: self.created_at.into(),
        }
    }
}
