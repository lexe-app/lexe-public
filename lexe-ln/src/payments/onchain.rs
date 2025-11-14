use anyhow::{Context, bail, ensure};
use bitcoin::Transaction;
use common::{
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    time::TimestampMs,
};
use lexe_api::{
    models::command::PayOnchainRequest,
    types::payments::{ClientPaymentId, LxPaymentId},
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    esplora::{TxConfQuery, TxConfStatus},
    payments::{PaymentMetadata, PaymentWithMetadata},
};

/// The number of confirmations a tx needs to before we consider it final.
pub(crate) const ONCHAIN_CONFIRMATION_THRESHOLD: u32 = 6;

// --- Onchain send --- //

// TODO(max): Separate out metadata fields
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnchainSendV2 {
    pub cid: ClientPaymentId,
    pub txid: LxTxid,
    // TODO(max): Add a serde helper to consensus encode the transaction before
    // serialization
    pub tx: Transaction,
    /// The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,
    pub priority: ConfirmationPriority,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainSendStatus,
    /// Set to `Some` when the payment is first persisted.
    pub created_at: Option<TimestampMs>,
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

impl OnchainSendV2 {
    // Event sources:
    // - `pay_onchain` API
    pub fn new(
        tx: Transaction,
        req: PayOnchainRequest,
        fees: Amount,
    ) -> PaymentWithMetadata<Self> {
        let os = Self {
            cid: req.cid,
            txid: LxTxid(tx.compute_txid()),
            tx,
            replacement: None,
            priority: req.priority,
            amount: req.amount,
            fees,
            status: OnchainSendStatus::Created,
            created_at: None,
            note: req.note,
            finalized_at: None,
        };

        // TODO(max): Populate metadata fields
        let id = LxPaymentId::OnchainSend(os.cid);
        let metadata = PaymentMetadata::empty(id);

        PaymentWithMetadata {
            payment: os,
            metadata,
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

    pub fn to_tx_conf_query(&self) -> anyhow::Result<TxConfQuery> {
        Ok(TxConfQuery {
            txid: self.txid,
            inputs: self
                .tx
                .input
                .iter()
                .map(|txin| txin.previous_output)
                .collect(),
            created_at: self
                .created_at
                .context(
                    "Payment should have been persisted (which sets created_at)
                     prior to appearing in `pending` map",
                )?
                .into(),
        })
    }
}

// --- Onchain receive --- //

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
    /// - It has been at least 14 days since we first detected this transaction.
    ///
    /// 14 days is the default `-mempoolexpiry` value in Bitcoin Core. It is
    /// likely that most nodes will have evicted our transaction from their
    /// mempool by now. There is a small chance that this transaction ends up
    /// getting confirmed, but we'll mark it as failed in our payments manager
    /// and move on, since this isn't security-critical; the user will still
    /// see the successful receive reflected in their wallet balance.
    Dropped,
}

#[cfg(test)]
mod test {
    use common::test_utils::{
        arbitrary, roundtrip::json_unit_enum_backwards_compat,
    };
    use lexe_api::models::command::PayOnchainRequest;
    use proptest::{
        arbitrary::{Arbitrary, any},
        option,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser = r#"["created","broadcasted","replacement_broadcasted","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainSendStatus>(expected_ser);

        let expected_ser = r#"["zeroconf","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainReceiveStatus>(expected_ser);
    }

    impl Arbitrary for OnchainSendV2 {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_tx = arbitrary::any_raw_tx();
            let any_req = any::<PayOnchainRequest>();
            let any_fees = any::<Amount>();
            let any_is_broadcasted = proptest::bool::weighted(0.8);
            // TODO(max): Make optional once payment_encryption_roundtrip tests
            // with PaymentV2 only. Currently must be non-optional because the
            // test exercises v2 → v1 conversion which requires created_at.
            let any_created_at = any::<TimestampMs>();
            let any_conf_status = option::weighted(0.8, any::<TxConfStatus>());

            // Generate valid `OnchainSend` instances by actually running
            // through the state machine.
            (
                any_tx,
                any_req,
                any_fees,
                any_created_at,
                any_is_broadcasted,
                any_conf_status,
            )
                .prop_map(
                    |(
                        tx,
                        req,
                        fees,
                        created_at,
                        is_broadcasted,
                        conf_status,
                    )| {
                        let mut pwm = OnchainSendV2::new(tx, req, fees);
                        // Set created_at for test purposes
                        pwm.payment.created_at = Some(created_at);
                        if !is_broadcasted {
                            return pwm.payment;
                        }
                        let os =
                            pwm.payment.broadcasted(&pwm.payment.txid).unwrap();
                        let mut pwm = PaymentWithMetadata {
                            payment: os,
                            metadata: pwm.metadata,
                        };
                        if let Some(conf_status) = conf_status
                            && let Some(os2) = pwm
                                .payment
                                .check_onchain_conf(conf_status)
                                .unwrap()
                        {
                            pwm.payment = os2;
                        }
                        pwm.payment
                    },
                )
                .boxed()
        }
    }
}
