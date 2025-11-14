use std::sync::Arc;

use anyhow::bail;
use bitcoin::Transaction;
use common::{
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    time::TimestampMs,
};
use lexe_api::types::payments::{ClientPaymentId, LxPaymentId};
use serde::{Deserialize, Serialize};

use crate::{
    esplora::{TxConfQuery, TxConfStatus},
    payments::{
        PaymentMetadata, PaymentWithMetadata,
        onchain::{
            ONCHAIN_CONFIRMATION_THRESHOLD, OnchainReceiveStatus,
            OnchainSendStatus, OnchainSendV2,
        },
    },
};

// --- Onchain send --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnchainSendV1 {
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

impl OnchainSendV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OnchainSend(self.cid)
    }
}

impl From<OnchainSendV1> for PaymentWithMetadata<OnchainSendV2> {
    fn from(v1: OnchainSendV1) -> Self {
        let id = v1.id();
        let note = v1.note.clone();

        let payment = OnchainSendV2 {
            cid: v1.cid,
            txid: v1.txid,
            tx: v1.tx,
            replacement: v1.replacement,
            priority: v1.priority,
            amount: v1.amount,
            fees: v1.fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            note: v1.note,
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id,
            invoice: None,
            offer: None,
            note,
        };

        Self { payment, metadata }
    }
}

impl From<PaymentWithMetadata<OnchainSendV2>> for OnchainSendV1 {
    fn from(pwm: PaymentWithMetadata<OnchainSendV2>) -> Self {
        // Intentionally destructure to ensure all fields are considered.
        let OnchainSendV2 {
            cid,
            txid,
            tx,
            replacement,
            priority,
            amount,
            fees,
            status,
            created_at,
            note,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            invoice: _,
            offer: _,
            // Both the v2 payment type and metadata contain the same field,
            // meaning the v2 payment type hasn't been migrated yet, meaning the
            // note field will always be in the v2 payment type. Once the field
            // is deleted from the v2 payment type, then the data will be stored
            // only in the metadata.
            note: _,
        } = pwm.metadata;

        Self {
            cid,
            txid,
            tx,
            replacement,
            priority,
            amount,
            fees,
            status,
            created_at: created_at.expect(
                "All payments data serialized as PaymentV1 has created_at, \
                 therefore this field is always Some(_)",
            ),
            note,
            finalized_at,
        }
    }
}

// --- Onchain receive --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnchainReceiveV1 {
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

impl OnchainReceiveV1 {
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
    use lexe_api::models::command::PayOnchainRequest;
    use proptest::{
        arbitrary::{Arbitrary, any},
        option,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::payments::onchain::OnchainSendV2;

    impl Arbitrary for OnchainSendV1 {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_tx = arbitrary::any_raw_tx();
            let any_req = any::<PayOnchainRequest>();
            let any_fees = any::<Amount>();
            let any_is_broadcasted = proptest::bool::weighted(0.8);
            // Generate a non-optional timestamp since all PaymentV1 data has
            // created_at. We wrap it in Some() when setting the v2 field.
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
                        let mut oswm = OnchainSendV2::new(tx, req, fees);
                        // Set created_at for test purposes
                        oswm.payment.created_at = Some(created_at);
                        if !is_broadcasted {
                            return OnchainSendV1::from(oswm);
                        }
                        let os = oswm
                            .payment
                            .broadcasted(&oswm.payment.txid)
                            .unwrap();
                        let mut oswm = PaymentWithMetadata {
                            payment: os,
                            metadata: oswm.metadata,
                        };
                        if let Some(conf_status) = conf_status {
                            if let Some(os2) = oswm
                                .payment
                                .check_onchain_conf(conf_status)
                                .unwrap()
                            {
                                oswm.payment = os2;
                            }
                            OnchainSendV1::from(oswm)
                        } else {
                            OnchainSendV1::from(oswm)
                        }
                    },
                )
                .boxed()
        }
    }

    impl Arbitrary for OnchainReceiveV1 {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let tx = arbitrary::any_raw_tx();
            let amount = any::<Amount>();
            let conf_status = option::weighted(0.8, any::<TxConfStatus>());

            // Generate valid `OnchainReceive` instances by actually running
            // through the state machine.
            (tx, amount, conf_status)
                .prop_map(|(tx, amount, conf_status)| {
                    let orp = OnchainReceiveV1::new(Arc::new(tx), amount);
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
