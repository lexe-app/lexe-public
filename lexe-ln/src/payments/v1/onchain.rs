use std::{collections::HashSet, sync::Arc};

use anyhow::Context;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    time::TimestampMs,
};
use lexe_api::types::payments::{ClientPaymentId, LxPaymentId, PaymentClass};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::payments::{
    PaymentMetadata, PaymentWithMetadata,
    onchain::{
        OnchainReceiveStatus, OnchainReceiveV2, OnchainSendStatus,
        OnchainSendV2,
    },
};

// --- Onchain send --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainSendV1 {
    pub cid: ClientPaymentId,
    pub txid: LxTxid,
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_raw_tx().prop_map(Arc::new)")
    )]
    pub tx: Arc<bitcoin::Transaction>,
    /// The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,
    pub priority: ConfirmationPriority,
    pub amount: Amount,
    pub fees: Amount,
    pub status: OnchainSendStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
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

        let payment = OnchainSendV2 {
            cid: v1.cid,
            txid: v1.txid,
            class: PaymentClass::Onchain,
            tx: v1.tx,
            amount: v1.amount,
            onchain_fee: v1.fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id,
            related_ids: HashSet::new(),
            address: None, // v1 doesn't store address separately
            invoice: None,
            offer: None,
            note: v1.note,
            payer_name: None,
            payer_note: None,
            priority: Some(v1.priority),
            quantity: None,
            replacement_txid: v1.replacement,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<OnchainSendV2>> for OnchainSendV1 {
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<OnchainSendV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let OnchainSendV2 {
            cid,
            txid,
            class: _,
            tx,
            amount,
            onchain_fee,
            status,
            created_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            related_ids: _,
            address: _,
            invoice: _,
            offer: _,
            note,
            payer_name: _,
            payer_note: _,
            priority,
            quantity: _,
            replacement_txid: replacement,
        } = pwm.metadata;

        Ok(Self {
            cid,
            txid,
            tx,
            replacement,
            priority: priority.context("Missing priority")?,
            amount,
            fees: onchain_fee,
            status,
            created_at: created_at.context("Missing created_at")?,
            note,
            finalized_at,
        })
    }
}

// --- Onchain receive --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainReceiveV1 {
    pub txid: LxTxid,
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_raw_tx().prop_map(Arc::new)")
    )]
    pub tx: Arc<bitcoin::Transaction>,
    /// The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,
    pub amount: Amount,
    pub status: OnchainReceiveStatus,
    pub created_at: TimestampMs,
    /// An optional personal note for this payment. Is set to [`None`] when the
    /// payment is first detected, but the user can add or modify it later.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub note: Option<String>,
    pub finalized_at: Option<TimestampMs>,
}

impl OnchainReceiveV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OnchainRecv(self.txid)
    }
}

impl From<OnchainReceiveV1> for PaymentWithMetadata<OnchainReceiveV2> {
    fn from(v1: OnchainReceiveV1) -> Self {
        let payment = OnchainReceiveV2 {
            txid: v1.txid,
            class: PaymentClass::Onchain,
            tx: v1.tx,
            amount: v1.amount,
            status: v1.status,
            created_at: Some(v1.created_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: payment.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: None,
            offer: None,
            note: v1.note,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: v1.replacement,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<OnchainReceiveV2>> for OnchainReceiveV1 {
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<OnchainReceiveV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let OnchainReceiveV2 {
            txid,
            class: _,
            tx,
            amount,
            status,
            created_at,
            finalized_at,
        } = pwm.payment;

        Ok(Self {
            txid,
            tx,
            replacement: pwm.metadata.replacement_txid,
            amount,
            status,
            created_at: created_at.context("Missing created_at")?,
            note: pwm.metadata.note,
            finalized_at,
        })
    }
}
