//! Lexe payments types and logic.
//!
//! This module is the 'complex' counterpart to the simpler types exposed in
//! [`common::ln::payments`].

use anyhow::Context;
use common::{
    aes::AesMasterKey,
    ln::{
        amount::Amount,
        hashes::LxTxid,
        invoice::LxInvoice,
        payments::{
            BasicPayment, DbPayment, LxPaymentId, PaymentDirection,
            PaymentIndex, PaymentKind, PaymentStatus,
        },
    },
    rng::Crng,
    time::TimestampMs,
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::payments::{
    inbound::{
        InboundInvoicePayment, InboundInvoicePaymentStatus,
        InboundSpontaneousPayment, InboundSpontaneousPaymentStatus,
    },
    onchain::{
        OnchainReceive, OnchainReceiveStatus, OnchainSend, OnchainSendStatus,
    },
    outbound::{
        OutboundInvoicePayment, OutboundInvoicePaymentStatus,
        OutboundSpontaneousPayment, OutboundSpontaneousPaymentStatus,
    },
};

/// Inbound Lightning payments.
pub mod inbound;
/// `PaymentsManager`.
pub mod manager;
/// On-chain payment types and state machines.
pub mod onchain;
/// Outbound Lightning payments.
pub mod outbound;

// --- The top-level payment type --- //

/// The top level [`Payment`] type which abstracts over all types of payments,
/// including both onchain and off-chain (Lightning) payments.
///
/// NOTE: Everything in this enum impls [`Serialize`] and [`Deserialize`], so be
/// mindful of backwards compatibility.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum Payment {
    OnchainSend(OnchainSend),
    OnchainReceive(OnchainReceive),
    // TODO(max): Implement SpliceIn
    // TODO(max): Implement SpliceOut
    InboundInvoice(InboundInvoicePayment),
    InboundSpontaneous(InboundSpontaneousPayment),
    OutboundInvoice(OutboundInvoicePayment),
    OutboundSpontaneous(OutboundSpontaneousPayment),
}

/// Serializes a given payment to JSON and encrypts the payment under the given
/// [`AesMasterKey`], returning the [`DbPayment`] which can be persisted.
pub fn encrypt(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    payment: &Payment,
) -> DbPayment {
    // Serialize the payment as JSON bytes.
    let aad = &[];
    let data_size_hint = None;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|mut_vec_u8| {
        serde_json::to_writer(mut_vec_u8, payment)
            .expect("Payment serialization always succeeds")
    };

    // Encrypt.
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPayment {
        created_at: payment.created_at().as_i64(),
        id: payment.id().to_string(),
        status: payment.status().to_string(),
        data,
    }
}

/// Given a [`DbPayment`], attempts to decrypt the associated ciphertext using
/// the given [`AesMasterKey`], returning the deserialized [`Payment`].
pub fn decrypt(
    vfs_master_key: &AesMasterKey,
    db_payment: DbPayment,
) -> anyhow::Result<Payment> {
    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, db_payment.data)
        .context("Could not decrypt Payment")?;

    serde_json::from_slice::<Payment>(plaintext_bytes.as_slice())
        .context("Could not deserialize Payment")
}

// --- Specific payment type -> top-level Payment types --- //

impl From<OnchainSend> for Payment {
    fn from(p: OnchainSend) -> Self {
        Self::OnchainSend(p)
    }
}
impl From<OnchainReceive> for Payment {
    fn from(p: OnchainReceive) -> Self {
        Self::OnchainReceive(p)
    }
}
impl From<InboundInvoicePayment> for Payment {
    fn from(p: InboundInvoicePayment) -> Self {
        Self::InboundInvoice(p)
    }
}
impl From<InboundSpontaneousPayment> for Payment {
    fn from(p: InboundSpontaneousPayment) -> Self {
        Self::InboundSpontaneous(p)
    }
}
impl From<OutboundInvoicePayment> for Payment {
    fn from(p: OutboundInvoicePayment) -> Self {
        Self::OutboundInvoice(p)
    }
}
impl From<OutboundSpontaneousPayment> for Payment {
    fn from(p: OutboundSpontaneousPayment) -> Self {
        Self::OutboundSpontaneous(p)
    }
}

// --- Payment -> BasicPayment --- //

impl From<Payment> for BasicPayment {
    fn from(p: Payment) -> Self {
        Self {
            index: p.index(),
            kind: p.kind(),
            direction: p.direction(),
            invoice: p.invoice(),
            txid: p.txid(),
            replacement: p.replacement(),
            amount: p.amount(),
            fees: p.fees(),
            status: p.status(),
            status_str: p.status_str().to_owned(),
            note: p.note().map(|s| s.to_owned()),
            finalized_at: p.finalized_at(),
        }
    }
}

// --- impl Payment --- //

impl Payment {
    pub fn index(&self) -> PaymentIndex {
        PaymentIndex {
            created_at: self.created_at(),
            id: self.id(),
        }
    }

    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::OnchainSend(os) => LxPaymentId::OnchainSend(os.cid),
            Self::OnchainReceive(or) => LxPaymentId::OnchainRecv(or.txid),
            Self::InboundInvoice(iip) => LxPaymentId::Lightning(iip.hash),
            Self::InboundSpontaneous(isp) => LxPaymentId::Lightning(isp.hash),
            Self::OutboundInvoice(oip) => LxPaymentId::Lightning(oip.hash),
            Self::OutboundSpontaneous(osp) => LxPaymentId::Lightning(osp.hash),
        }
    }

    /// Whether this is an onchain payment, LN invoice payment, etc.
    pub fn kind(&self) -> PaymentKind {
        match self {
            Self::OnchainSend(_) => PaymentKind::Onchain,
            Self::OnchainReceive(_) => PaymentKind::Onchain,
            Self::InboundInvoice(_) => PaymentKind::Invoice,
            Self::InboundSpontaneous(_) => PaymentKind::Spontaneous,
            Self::OutboundInvoice(_) => PaymentKind::Invoice,
            Self::OutboundSpontaneous(_) => PaymentKind::Spontaneous,
        }
    }

    /// Whether this payment is inbound or outbound. Useful for filtering.
    pub fn direction(&self) -> PaymentDirection {
        match self {
            Self::OnchainSend(_) => PaymentDirection::Outbound,
            Self::OnchainReceive(_) => PaymentDirection::Inbound,
            Self::InboundInvoice(_) => PaymentDirection::Inbound,
            Self::InboundSpontaneous(_) => PaymentDirection::Inbound,
            Self::OutboundInvoice(_) => PaymentDirection::Outbound,
            Self::OutboundSpontaneous(_) => PaymentDirection::Outbound,
        }
    }

    /// Returns the invoice corresponding to this payment, if there is one.
    pub fn invoice(&self) -> Option<LxInvoice> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(InboundInvoicePayment { invoice, .. }) =>
                Some(*invoice.clone()),
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(OutboundInvoicePayment {
                invoice, ..
            }) => Some(*invoice.clone()),
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the original txid, if there is one.
    pub fn txid(&self) -> Option<LxTxid> {
        match self {
            Self::OnchainSend(OnchainSend { txid, .. }) => Some(*txid),
            Self::OnchainReceive(OnchainReceive { txid, .. }) => Some(*txid),
            Self::InboundInvoice(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the txid of the replacement tx, if there is one.
    pub fn replacement(&self) -> Option<LxTxid> {
        match self {
            Self::OnchainSend(OnchainSend { replacement, .. }) => *replacement,
            Self::OnchainReceive(OnchainReceive { replacement, .. }) =>
                *replacement,
            Self::InboundInvoice(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, we return the amount
    ///   we received.
    /// - If this is a pending or failed inbound inbound invoice payment, we
    ///   return the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always returned.
    pub fn amount(&self) -> Option<Amount> {
        match self {
            Self::OnchainSend(OnchainSend { amount, .. }) => Some(*amount),
            Self::OnchainReceive(OnchainReceive { amount, .. }) =>
                Some(*amount),
            Self::InboundInvoice(InboundInvoicePayment {
                invoice_amount,
                recvd_amount,
                ..
            }) => recvd_amount.or(*invoice_amount),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                amount,
                ..
            }) => Some(*amount),
            Self::OutboundInvoice(OutboundInvoicePayment {
                amount, ..
            }) => Some(*amount),
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                amount,
                ..
            }) => Some(*amount),
        }
    }

    /// The fees paid or expected to be paid for this payment.
    pub fn fees(&self) -> Amount {
        match self {
            Self::OnchainSend(OnchainSend { fees, .. }) => *fees,
            // We don't pay anything to receive money onchain
            Self::OnchainReceive(OnchainReceive { .. }) => Amount::ZERO,
            Self::InboundInvoice(InboundInvoicePayment {
                onchain_fees,
                ..
            }) => onchain_fees.unwrap_or(Amount::from_msat(0)),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                onchain_fees,
                ..
            }) => onchain_fees.unwrap_or(Amount::from_msat(0)),
            Self::OutboundInvoice(OutboundInvoicePayment { fees, .. }) => *fees,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                fees,
                ..
            }) => *fees,
        }
    }

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    pub fn status(&self) -> PaymentStatus {
        match self {
            Self::OnchainSend(OnchainSend { status, .. }) =>
                PaymentStatus::from(*status),
            Self::OnchainReceive(OnchainReceive { status, .. }) =>
                PaymentStatus::from(*status),
            Self::InboundInvoice(InboundInvoicePayment { status, .. }) =>
                PaymentStatus::from(*status),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                status,
                ..
            }) => PaymentStatus::from(*status),
            Self::OutboundInvoice(OutboundInvoicePayment {
                status, ..
            }) => PaymentStatus::from(*status),
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                status,
                ..
            }) => PaymentStatus::from(*status),
        }
    }

    /// Get the payment status as a human-readable `&'static str`
    pub fn status_str(&self) -> &str {
        match self {
            Self::OnchainSend(OnchainSend { status, .. }) => status.as_str(),
            Self::OnchainReceive(OnchainReceive { status, .. }) =>
                status.as_str(),
            Self::InboundInvoice(InboundInvoicePayment { status, .. }) =>
                status.as_str(),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                status,
                ..
            }) => status.as_str(),
            Self::OutboundInvoice(OutboundInvoicePayment {
                status,
                failure,
                ..
            }) => failure
                .map(|f| f.as_str())
                .unwrap_or_else(|| status.as_str()),
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                status,
                ..
            }) => status.as_str(),
        }
    }

    /// Get the payment note.
    pub fn note(&self) -> Option<&str> {
        match self {
            Self::OnchainSend(OnchainSend { note, .. }) => note,
            Self::OnchainReceive(OnchainReceive { note, .. }) => note,
            Self::InboundInvoice(InboundInvoicePayment { note, .. }) => note,
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                note,
                ..
            }) => note,
            Self::OutboundInvoice(OutboundInvoicePayment { note, .. }) => note,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                note,
                ..
            }) => note,
        }
        .as_ref()
        .map(|s| s.as_str())
    }

    /// Set the payment note to a new value.
    pub fn set_note(&mut self, note: Option<String>) {
        let mut_ref_note = match self {
            Self::OnchainSend(OnchainSend { note, .. }) => note,
            Self::OnchainReceive(OnchainReceive { note, .. }) => note,
            Self::InboundInvoice(InboundInvoicePayment { note, .. }) => note,
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                note,
                ..
            }) => note,
            Self::OutboundInvoice(OutboundInvoicePayment { note, .. }) => note,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                note,
                ..
            }) => note,
        };

        *mut_ref_note = note;
    }

    /// When this payment was created.
    pub fn created_at(&self) -> TimestampMs {
        match self {
            Self::OnchainSend(OnchainSend { created_at, .. }) => *created_at,
            Self::OnchainReceive(OnchainReceive { created_at, .. }) =>
                *created_at,
            Self::InboundInvoice(InboundInvoicePayment {
                created_at, ..
            }) => *created_at,
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundInvoice(OutboundInvoicePayment {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                created_at,
                ..
            }) => *created_at,
        }
    }

    /// When this payment was completed or failed.
    pub fn finalized_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(OnchainSend { finalized_at, .. }) =>
                *finalized_at,
            Self::OnchainReceive(OnchainReceive { finalized_at, .. }) =>
                *finalized_at,
            Self::InboundInvoice(InboundInvoicePayment {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundInvoice(OutboundInvoicePayment {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                finalized_at,
                ..
            }) => *finalized_at,
        }
    }

    pub(crate) fn assert_invariants(&self) {
        // Payments should have a finalized_at() iff it has finalized.
        use PaymentStatus::*;
        match self.status() {
            Pending => assert!(self.finalized_at().is_none()),
            Completed | Failed => assert!(self.finalized_at().is_some()),
        }
    }
}

// --- Payment-specific status -> General PaymentStatus  --- //

impl From<OnchainSendStatus> for PaymentStatus {
    fn from(specific_status: OnchainSendStatus) -> Self {
        match specific_status {
            OnchainSendStatus::Created => Self::Pending,
            OnchainSendStatus::Broadcasted => Self::Pending,
            OnchainSendStatus::PartiallyConfirmed => Self::Pending,
            OnchainSendStatus::ReplacementBroadcasted => Self::Pending,
            OnchainSendStatus::PartiallyReplaced => Self::Pending,
            OnchainSendStatus::FullyConfirmed => Self::Completed,
            OnchainSendStatus::FullyReplaced => Self::Failed,
            OnchainSendStatus::Dropped => Self::Failed,
        }
    }
}

impl From<OnchainReceiveStatus> for PaymentStatus {
    fn from(specific_status: OnchainReceiveStatus) -> Self {
        match specific_status {
            OnchainReceiveStatus::Zeroconf => Self::Pending,
            OnchainReceiveStatus::PartiallyConfirmed => Self::Pending,
            OnchainReceiveStatus::PartiallyReplaced => Self::Pending,
            OnchainReceiveStatus::FullyConfirmed => Self::Completed,
            OnchainReceiveStatus::FullyReplaced => Self::Failed,
            OnchainReceiveStatus::Dropped => Self::Failed,
        }
    }
}

impl From<InboundInvoicePaymentStatus> for PaymentStatus {
    fn from(specific_status: InboundInvoicePaymentStatus) -> Self {
        match specific_status {
            InboundInvoicePaymentStatus::InvoiceGenerated => Self::Pending,
            InboundInvoicePaymentStatus::Claiming => Self::Pending,
            InboundInvoicePaymentStatus::Completed => Self::Completed,
            InboundInvoicePaymentStatus::Expired => Self::Failed,
        }
    }
}

impl From<InboundSpontaneousPaymentStatus> for PaymentStatus {
    fn from(specific_status: InboundSpontaneousPaymentStatus) -> Self {
        match specific_status {
            InboundSpontaneousPaymentStatus::Claiming => Self::Pending,
            InboundSpontaneousPaymentStatus::Completed => Self::Completed,
        }
    }
}

impl From<OutboundInvoicePaymentStatus> for PaymentStatus {
    fn from(specific_status: OutboundInvoicePaymentStatus) -> Self {
        match specific_status {
            OutboundInvoicePaymentStatus::Pending => Self::Pending,
            OutboundInvoicePaymentStatus::Abandoning => Self::Pending,
            OutboundInvoicePaymentStatus::Completed => Self::Completed,
            OutboundInvoicePaymentStatus::Failed => Self::Failed,
        }
    }
}

impl From<OutboundSpontaneousPaymentStatus> for PaymentStatus {
    fn from(specific_status: OutboundSpontaneousPaymentStatus) -> Self {
        match specific_status {
            OutboundSpontaneousPaymentStatus::Pending => Self::Pending,
            OutboundSpontaneousPaymentStatus::Completed => Self::Completed,
            OutboundSpontaneousPaymentStatus::Failed => Self::Failed,
        }
    }
}

// --- Use as_str() to get a human-readable payment status &str --- //

impl OnchainSendStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Created => "created",
            Self::Broadcasted => "broadcasted",
            Self::PartiallyConfirmed =>
                "partially confirmed (1-5 confirmations)",
            Self::ReplacementBroadcasted => "being replaced",
            Self::PartiallyReplaced =>
                "being replaced (replacement has 1-5 confirmations)",
            Self::FullyConfirmed => "fully confirmed (6+ confirmations)",
            Self::FullyReplaced =>
                "fully replaced (replacement has 6+ confirmations)",
            Self::Dropped => "dropped from mempool",
        }
    }
}

impl OnchainReceiveStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Zeroconf => "in mempool awaiting confirmations",
            Self::PartiallyConfirmed =>
                "partially confirmed (1-5 confirmations)",
            Self::PartiallyReplaced =>
                "being replaced (replacement has 1-5 confirmations)",
            Self::FullyConfirmed => "fully confirmed (6+ confirmations)",
            Self::FullyReplaced =>
                "fully replaced (replacement has 6+ confirmations)",
            Self::Dropped => "dropped from mempool",
        }
    }
}

impl InboundInvoicePaymentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::InvoiceGenerated => "invoice generated",
            Self::Claiming => "claiming",
            Self::Completed => "completed",
            Self::Expired => "invoice expired",
        }
    }
}

impl InboundSpontaneousPaymentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Claiming => "claiming",
            Self::Completed => "completed",
        }
    }
}

impl OutboundInvoicePaymentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Abandoning => "abandoning",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl OutboundSpontaneousPaymentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[cfg(test)]
mod test {
    use common::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::{
        arbitrary::any, prelude::Strategy, prop_assert_eq, proptest,
        test_runner::Config,
    };

    use super::*;

    // Generate serialized `BasicPayment` sample json data:
    // ```bash
    // $ cargo test -p lexe-ln -- gen_basic_payment_sample_data --ignored --nocapture
    // ```
    // NOTE: this lives here b/c `common` can't depend on `lexe-ln`.
    #[test]
    #[ignore]
    fn gen_basic_payment_sample_data() {
        let mut rng = FastRng::from_u64(202503031636);
        const N: usize = 3;

        // generate `N` samples for each variant to ensure we get full coverage
        let strategies = vec![
            (
                "OnchainSend",
                any::<OnchainSend>().prop_map(Payment::OnchainSend).boxed(),
            ),
            (
                "OnchainReceive",
                any::<OnchainReceive>()
                    .prop_map(Payment::OnchainReceive)
                    .boxed(),
            ),
            (
                "InboundInvoice",
                any::<InboundInvoicePayment>()
                    .prop_map(Payment::InboundInvoice)
                    .boxed(),
            ),
            (
                "InboundSpontaneous",
                any::<InboundSpontaneousPayment>()
                    .prop_map(Payment::InboundSpontaneous)
                    .boxed(),
            ),
            (
                "OutboundInvoice",
                any::<OutboundInvoicePayment>()
                    .prop_map(Payment::OutboundInvoice)
                    .boxed(),
            ),
            (
                "OutboundSpontaneous",
                any::<OutboundSpontaneousPayment>()
                    .prop_map(Payment::OutboundSpontaneous)
                    .boxed(),
            ),
        ];

        for (name, strat) in strategies {
            println!("--- {name}");
            for mut value in arbitrary::gen_value_iter(&mut rng, strat).take(N)
            {
                // clean long annoying unicode notes
                if value.note().is_some() {
                    value.set_note(Some("foo bar".to_owned()));
                }

                // serialize app BasicPayment
                let value = BasicPayment::from(value);
                let json = serde_json::to_string(&value).unwrap();
                println!("{json}");
            }
        }
    }

    #[test]
    fn top_level_payment_serde_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<Payment>();
    }

    #[test]
    fn low_level_payments_serde_roundtrips() {
        use roundtrip::json_value_custom;
        let config = Config::with_cases(16);
        json_value_custom(any::<OnchainSend>(), config.clone());
        json_value_custom(any::<OnchainReceive>(), config.clone());
        // TODO(max): Add SpliceIn
        // TODO(max): Add SpliceOut
        json_value_custom(any::<InboundInvoicePayment>(), config.clone());
        json_value_custom(any::<InboundSpontaneousPayment>(), config.clone());
        json_value_custom(any::<OutboundInvoicePayment>(), config.clone());
        json_value_custom(any::<OutboundSpontaneousPayment>(), config);
    }

    #[test]
    fn payment_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            p1 in any::<Payment>(),
        )| {
            let encrypted = super::encrypt(&mut rng, &vfs_master_key, &p1);
            let p2 = super::decrypt(&vfs_master_key, encrypted).unwrap();
            prop_assert_eq!(p1, p2);
        })
    }

    #[test]
    fn payment_id_equivalence() {
        let cfg = Config::with_cases(100);

        proptest!(cfg, |(payment: Payment)| {
            let id = match &payment {
                Payment::OnchainSend(x) => x.id(),
                Payment::OnchainReceive(x) => x.id(),
                Payment::InboundInvoice(x) => x.id(),
                Payment::InboundSpontaneous(x) => x.id(),
                Payment::OutboundInvoice(x) => x.id(),
                Payment::OutboundSpontaneous(x) => x.id(),
            };
            prop_assert_eq!(id, payment.id());
        });
    }
}
