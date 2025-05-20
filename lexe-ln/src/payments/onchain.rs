use std::sync::Arc;

use anyhow::{bail, ensure};
use bitcoin::Transaction;
use common::{
    ln::{
        amount::Amount,
        hashes::LxTxid,
        payments::{ClientPaymentId, LxPaymentId},
        priority::ConfirmationPriority,
    },
    time::TimestampMs,
};
use lexe_api::models::command::PayOnchainRequest;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::esplora::{TxConfQuery, TxConfStatus};

/// The number of confirmations a tx needs to before we consider it final.
const ONCHAIN_CONFIRMATION_THRESHOLD: u32 = 6;

// --- Onchain send --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnchainSend {
    pub cid: ClientPaymentId,
    pub txid: LxTxid,
    pub tx: Transaction,
    /// The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,
    pub priority: ConfirmationPriority,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainSendStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment.
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
pub enum OnchainSendStatus {
    /// (Pending, not broadcasted) The tx has been created and signed but
    /// hasn't been broadcasted yet.
    //
    // TODO(phlip9): handle the case where we create a new `OnchainSend` but
    // crash before we broadcast the tx.
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
    // Event sources:
    // - `pay_onchain` API
    pub fn new(tx: Transaction, req: PayOnchainRequest, fees: Amount) -> Self {
        Self {
            cid: req.cid,
            txid: LxTxid(tx.compute_txid()),
            tx,
            replacement: None,
            priority: req.priority,
            amount: req.amount,
            fees,
            status: OnchainSendStatus::Created,
            created_at: TimestampMs::now(),
            note: req.note,
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OnchainSend(self.cid)
    }

    // Event sources:
    // - `pay_onchain` API
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

    // Event sources:
    // - `PaymentsManager::spawn_onchain_confs_checker` task
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

        let new_status = match &conf_status {
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
                if confs < &ONCHAIN_CONFIRMATION_THRESHOLD {
                    PartiallyConfirmed
                } else {
                    FullyConfirmed
                },
            TxConfStatus::HasReplacement { confs, .. } =>
                if confs < &ONCHAIN_CONFIRMATION_THRESHOLD {
                    PartiallyReplaced
                } else {
                    FullyReplaced
                },
            TxConfStatus::Dropped => Dropped,
        };
        let new_replacement = match conf_status {
            TxConfStatus::HasReplacement { rp_txid, .. } => Some(rp_txid),
            _ => None,
        };

        // To prevent redundantly repersisting the same data, return Some(..)
        // only if the state has actually changed.
        if (self.status == new_status) && (self.replacement == new_replacement)
        {
            Ok(None)
        } else {
            let mut clone = self.clone();
            clone.status = new_status;
            clone.replacement = new_replacement;

            if matches!(new_status, FullyConfirmed | FullyReplaced | Dropped) {
                clone.finalized_at = Some(TimestampMs::now());
            }

            Ok(Some(clone))
        }
    }

    pub fn to_tx_conf_query(&self) -> TxConfQuery {
        TxConfQuery {
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
pub struct OnchainReceive {
    pub txid: LxTxid,
    pub tx: Arc<Transaction>,
    /// The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,
    pub amount: Amount,
    pub status: OnchainReceiveStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment. Is set to [`None`] when the
    /// payment is first detected, but the user can add or modify it later.
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
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
    // Event sources:
    // - `PaymentsManager::spawn_onchain_recv_checker` task
    pub(crate) fn new(tx: Arc<Transaction>, amount: Amount) -> Self {
        Self {
            txid: LxTxid(tx.compute_txid()),
            tx,
            replacement: None,
            amount,
            // Start at zeroconf and let the checker update it later.
            status: OnchainReceiveStatus::Zeroconf,
            created_at: TimestampMs::now(),
            note: None,
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OnchainRecv(self.txid)
    }

    // Event sources:
    // - `PaymentsManager::spawn_onchain_confs_checker` task
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

        let new_status = match &conf_status {
            TxConfStatus::ZeroConf => Zeroconf,
            TxConfStatus::InBestChain { confs } =>
                if confs < &ONCHAIN_CONFIRMATION_THRESHOLD {
                    PartiallyConfirmed
                } else {
                    FullyConfirmed
                },
            TxConfStatus::HasReplacement { confs, .. } =>
                if confs < &ONCHAIN_CONFIRMATION_THRESHOLD {
                    PartiallyReplaced
                } else {
                    FullyReplaced
                },
            TxConfStatus::Dropped => Dropped,
        };
        let new_replacement = match conf_status {
            TxConfStatus::HasReplacement { rp_txid, .. } => Some(rp_txid),
            _ => None,
        };

        // To prevent redundantly repersisting the same data, return Some(..)
        // only if the state has actually changed.
        if (self.status == new_status) && (self.replacement == new_replacement)
        {
            Ok(None)
        } else {
            let mut clone = self.clone();
            clone.status = new_status;
            clone.replacement = new_replacement;

            if matches!(new_status, FullyConfirmed | FullyReplaced | Dropped) {
                clone.finalized_at = Some(TimestampMs::now());
            }

            Ok(Some(clone))
        }
    }

    pub fn to_tx_conf_query(&self) -> TxConfQuery {
        TxConfQuery {
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

#[cfg(test)]
mod arb {
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for OnchainSend {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let tx = arbitrary::any_raw_tx();
            let req = any::<PayOnchainRequest>();
            let fees = any::<Amount>();
            let is_broadcasted = proptest::bool::weighted(0.8);
            let conf_status =
                proptest::option::weighted(0.8, any::<TxConfStatus>());

            // Generate valid `OnchainSend` instances by actually running
            // through the state machine.
            (tx, req, fees, is_broadcasted, conf_status)
                .prop_map(|(tx, req, fees, is_broadcasted, conf_status)| {
                    let os = OnchainSend::new(tx, req, fees);
                    if !is_broadcasted {
                        return os;
                    }
                    let os = os.broadcasted(&os.txid).unwrap();
                    if let Some(conf_status) = conf_status {
                        os.check_onchain_conf(conf_status)
                            .unwrap()
                            .unwrap_or(os)
                    } else {
                        os
                    }
                })
                .boxed()
        }
    }

    impl Arbitrary for OnchainReceive {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let tx = arbitrary::any_raw_tx();
            let amount = any::<Amount>();
            let conf_status =
                proptest::option::weighted(0.8, any::<TxConfStatus>());

            // Generate valid `OnchainReceive` instances by actually running
            // through the state machine.
            (tx, amount, conf_status)
                .prop_map(|(tx, amount, conf_status)| {
                    let orp = OnchainReceive::new(Arc::new(tx), amount);
                    if let Some(conf_status) = conf_status {
                        orp.check_onchain_conf(conf_status)
                            .unwrap()
                            .unwrap_or(orp)
                    } else {
                        orp
                    }
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip::json_unit_enum_backwards_compat;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser = r#"["created","broadcasted","replacement_broadcasted","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainSendStatus>(expected_ser);

        let expected_ser = r#"["zeroconf","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainReceiveStatus>(expected_ser);
    }
}
