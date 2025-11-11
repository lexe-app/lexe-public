use common::{
    ln::{amount::Amount, hashes::LxTxid},
    time::TimestampMs,
};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        BasicPaymentV1, BasicPaymentV2, LxOfferId, LxPaymentId,
        PaymentCreatedIndex, PaymentDirection, PaymentKind, PaymentStatus,
    },
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::payments::v1::{
    inbound::{
        InboundInvoicePaymentV1, InboundOfferReusablePaymentV1,
        InboundSpontaneousPaymentV1,
    },
    onchain::{OnchainReceiveV1, OnchainSendV1},
    outbound::{
        OutboundInvoicePaymentV1, OutboundOfferPaymentV1,
        OutboundSpontaneousPaymentV1,
    },
};

/// Inbound Lightning payments.
pub mod inbound;
/// On-chain payment types and state machines.
pub mod onchain;
/// Outbound Lightning payments.
pub mod outbound;

// --- The top-level payment type --- //

/// The top level `Payment` type which abstracts over all types of payments,
/// including both onchain and off-chain (Lightning) payments.
///
/// Each variant in `Payment` typically implements a state machine that
/// ingests events from [`PaymentsManager`] to transition between states in
/// that payment type's lifecycle.
///
/// For example, we create an [`OnchainSendV1`] payment in its initial state,
/// `Created`. After we successfully broadcast the tx, the payment transitions
/// to `Broadcasted`. Once the tx confirms, the payment transitions to
/// `PartiallyConfirmed`, then `FullyConfirmed` with 6+ confs.
///
/// ### State machine idempotency
///
/// In certain situations, payment state machines updates have to be idempotent
/// to handle replays of (1) the latest `EventHandler` event, or, for certain
/// types of events, (2) any previous (relevant) event.
///
/// We experience (1) if the node crashes while the `EventHandler` is
/// processing a payment event, specifically after the payment update is saved
/// but before the event log persists. In this case, the event will be replayed
/// on next startup.
///
/// For (2), the `EventHandler` may replay certain events that return a
/// [`Replay`] error. These will keep getting replayed until the event returns
/// `Ok` or [`Discard`]. These may even be replayed long after the payment
/// has finalized.
///
/// ### Backwards compatibility
///
/// NOTE: Everything in this enum impls [`Serialize`] and [`Deserialize`], so be
/// mindful of backwards compatibility.
///
/// [`PaymentsManager`]: crate::payments::manager::PaymentsManager
/// [`Replay`]: crate::event::EventHandleError::Replay
/// [`Discard`]: crate::event::EventHandleError::Discard
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum PaymentV1 {
    OnchainSend(OnchainSendV1),
    OnchainReceive(OnchainReceiveV1),
    // TODO(max): Implement SpliceIn
    // TODO(max): Implement SpliceOut
    InboundInvoice(InboundInvoicePaymentV1),
    // TODO(phlip9): InboundOffer (single-use)
    // Added in `node-v0.7.8`
    InboundOfferReusable(InboundOfferReusablePaymentV1),
    InboundSpontaneous(InboundSpontaneousPaymentV1),
    OutboundInvoice(OutboundInvoicePaymentV1),
    // Added in `node-v0.7.8`
    OutboundOffer(OutboundOfferPaymentV1),
    OutboundSpontaneous(OutboundSpontaneousPaymentV1),
}

// --- Specific payment type -> top-level Payment types --- //

impl From<OnchainSendV1> for PaymentV1 {
    fn from(p: OnchainSendV1) -> Self {
        Self::OnchainSend(p)
    }
}
impl From<OnchainReceiveV1> for PaymentV1 {
    fn from(p: OnchainReceiveV1) -> Self {
        Self::OnchainReceive(p)
    }
}
impl From<InboundInvoicePaymentV1> for PaymentV1 {
    fn from(p: InboundInvoicePaymentV1) -> Self {
        Self::InboundInvoice(p)
    }
}
impl From<InboundOfferReusablePaymentV1> for PaymentV1 {
    fn from(p: InboundOfferReusablePaymentV1) -> Self {
        Self::InboundOfferReusable(p)
    }
}
impl From<InboundSpontaneousPaymentV1> for PaymentV1 {
    fn from(p: InboundSpontaneousPaymentV1) -> Self {
        Self::InboundSpontaneous(p)
    }
}
impl From<OutboundInvoicePaymentV1> for PaymentV1 {
    fn from(p: OutboundInvoicePaymentV1) -> Self {
        Self::OutboundInvoice(p)
    }
}
impl From<OutboundOfferPaymentV1> for PaymentV1 {
    fn from(p: OutboundOfferPaymentV1) -> Self {
        Self::OutboundOffer(p)
    }
}
impl From<OutboundSpontaneousPaymentV1> for PaymentV1 {
    fn from(p: OutboundSpontaneousPaymentV1) -> Self {
        Self::OutboundSpontaneous(p)
    }
}

// --- Payment -> BasicPaymentV1 --- //

impl From<PaymentV1> for BasicPaymentV1 {
    fn from(p: PaymentV1) -> Self {
        Self {
            index: p.index(),
            kind: p.kind(),
            direction: p.direction(),
            invoice: p.invoice(),
            offer_id: p.offer_id(),
            offer: p.offer(),
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

impl PaymentV1 {
    // Can't impl BasicPaymentV2::from_payment bc we don't want to move
    // `Payment` into `lexe-api-core`.
    pub fn into_basic_payment(
        self,
        created_at: TimestampMs,
        updated_at: TimestampMs,
        // TODO(max): We will add a PaymentMetadata param here later.
    ) -> BasicPaymentV2 {
        BasicPaymentV2 {
            id: self.id(),
            kind: self.kind(),
            direction: self.direction(),
            invoice: self.invoice(),
            offer_id: self.offer_id(),
            offer: self.offer(),
            txid: self.txid(),
            replacement: self.replacement(),
            amount: self.amount(),
            fees: self.fees(),
            status: self.status(),
            status_str: self.status_str().to_owned(),
            note: self.note().map(|s| s.to_owned()),
            finalized_at: self.finalized_at(),
            created_at,
            updated_at,
        }
    }

    pub fn index(&self) -> PaymentCreatedIndex {
        PaymentCreatedIndex {
            created_at: self.created_at(),
            id: self.id(),
        }
    }

    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::OnchainSend(os) => LxPaymentId::OnchainSend(os.cid),
            Self::OnchainReceive(or) => LxPaymentId::OnchainRecv(or.txid),
            Self::InboundInvoice(iip) => LxPaymentId::Lightning(iip.hash),
            Self::InboundOfferReusable(iorp) =>
                LxPaymentId::OfferRecvReusable(iorp.claim_id),
            Self::InboundSpontaneous(isp) => LxPaymentId::Lightning(isp.hash),
            Self::OutboundInvoice(oip) => LxPaymentId::Lightning(oip.hash),
            Self::OutboundOffer(oop) => LxPaymentId::OfferSend(oop.cid),
            Self::OutboundSpontaneous(osp) => LxPaymentId::Lightning(osp.hash),
        }
    }

    /// Whether this is an onchain payment, LN invoice payment, etc.
    pub fn kind(&self) -> PaymentKind {
        match self {
            Self::OnchainSend(_) => PaymentKind::Onchain,
            Self::OnchainReceive(_) => PaymentKind::Onchain,
            Self::InboundInvoice(_) => PaymentKind::Invoice,
            Self::InboundOfferReusable(_) => PaymentKind::Offer,
            Self::InboundSpontaneous(_) => PaymentKind::Spontaneous,
            Self::OutboundInvoice(_) => PaymentKind::Invoice,
            Self::OutboundOffer(_) => PaymentKind::Offer,
            Self::OutboundSpontaneous(_) => PaymentKind::Spontaneous,
        }
    }

    /// Whether this payment is inbound or outbound. Useful for filtering.
    pub fn direction(&self) -> PaymentDirection {
        match self {
            Self::OnchainSend(_) => PaymentDirection::Outbound,
            Self::OnchainReceive(_) => PaymentDirection::Inbound,
            Self::InboundInvoice(_) => PaymentDirection::Inbound,
            Self::InboundOfferReusable(_) => PaymentDirection::Inbound,
            Self::InboundSpontaneous(_) => PaymentDirection::Inbound,
            Self::OutboundInvoice(_) => PaymentDirection::Outbound,
            Self::OutboundOffer(_) => PaymentDirection::Outbound,
            Self::OutboundSpontaneous(_) => PaymentDirection::Outbound,
        }
    }

    /// Returns the BOLT11 invoice corresponding to this payment, if there is
    /// one.
    pub fn invoice(&self) -> Option<Box<LxInvoice>> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                invoice, ..
            }) => Some(invoice.clone()),
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                invoice, ..
            }) => Some(invoice.clone()),
            Self::OutboundOffer(_) => None,
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the id of the BOLT12 offer associated with this payment, if
    /// there is one.
    pub fn offer_id(&self) -> Option<LxOfferId> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(_) => None,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                offer_id,
                ..
            }) => Some(*offer_id),
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(OutboundOfferPaymentV1 { offer, .. }) =>
                Some(offer.id()),
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the BOLT12 offer associated with this payment, if there is one.
    pub fn offer(&self) -> Option<Box<LxOffer>> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(_) => None,
            // TODO(phlip9): out-of-line offer metadata storage
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(OutboundOfferPaymentV1 { offer, .. }) =>
                Some(offer.clone()),
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the original txid, if there is one.
    pub fn txid(&self) -> Option<LxTxid> {
        match self {
            Self::OnchainSend(OnchainSendV1 { txid, .. }) => Some(*txid),
            Self::OnchainReceive(OnchainReceiveV1 { txid, .. }) => Some(*txid),
            Self::InboundInvoice(_) => None,
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(_) => None,
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the txid of the replacement tx, if there is one.
    pub fn replacement(&self) -> Option<LxTxid> {
        match self {
            Self::OnchainSend(OnchainSendV1 { replacement, .. }) =>
                *replacement,
            Self::OnchainReceive(OnchainReceiveV1 { replacement, .. }) =>
                *replacement,
            Self::InboundInvoice(_) => None,
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(_) => None,
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
            Self::OnchainSend(OnchainSendV1 { amount, .. }) => Some(*amount),
            Self::OnchainReceive(OnchainReceiveV1 { amount, .. }) =>
                Some(*amount),
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                invoice_amount,
                recvd_amount,
                ..
            }) => recvd_amount.or(*invoice_amount),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                amount,
                ..
            }) => Some(*amount),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                amount,
                ..
            }) => Some(*amount),
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                amount, ..
            }) => Some(*amount),
            Self::OutboundOffer(OutboundOfferPaymentV1 { amount, .. }) =>
                Some(*amount),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                amount,
                ..
            }) => Some(*amount),
        }
    }

    /// The fees paid or expected to be paid for this payment.
    pub fn fees(&self) -> Amount {
        match self {
            Self::OnchainSend(OnchainSendV1 { fees, .. }) => *fees,
            // We don't pay anything to receive money onchain
            Self::OnchainReceive(OnchainReceiveV1 { .. }) => Amount::ZERO,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                onchain_fees,
                ..
            }) => onchain_fees.unwrap_or(Amount::from_msat(0)),
            Self::InboundOfferReusable(iorp) => iorp.fees(),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                onchain_fees,
                ..
            }) => onchain_fees.unwrap_or(Amount::from_msat(0)),
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                fees, ..
            }) => *fees,
            Self::OutboundOffer(OutboundOfferPaymentV1 { fees, .. }) => *fees,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                fees,
                ..
            }) => *fees,
        }
    }

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    pub fn status(&self) -> PaymentStatus {
        match self {
            Self::OnchainSend(OnchainSendV1 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::OnchainReceive(OnchainReceiveV1 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                status, ..
            }) => PaymentStatus::from(*status),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                status,
                ..
            }) => PaymentStatus::from(*status),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                status,
                ..
            }) => PaymentStatus::from(*status),
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                status, ..
            }) => PaymentStatus::from(*status),
            Self::OutboundOffer(OutboundOfferPaymentV1 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                status,
                ..
            }) => PaymentStatus::from(*status),
        }
    }

    /// Get the payment status as a human-readable `&'static str`
    pub fn status_str(&self) -> &str {
        match self {
            Self::OnchainSend(OnchainSendV1 { status, .. }) => status.as_str(),
            Self::OnchainReceive(OnchainReceiveV1 { status, .. }) =>
                status.as_str(),
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                status, ..
            }) => status.as_str(),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                status,
                ..
            }) => status.as_str(),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                status,
                ..
            }) => status.as_str(),
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                status,
                failure,
                ..
            }) => failure
                .map(|f| f.as_str())
                .unwrap_or_else(|| status.as_str()),
            Self::OutboundOffer(OutboundOfferPaymentV1 { status, .. }) =>
                status.as_str(),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                status,
                ..
            }) => status.as_str(),
        }
    }

    /// Get the payment note.
    pub fn note(&self) -> Option<&str> {
        match self {
            Self::OnchainSend(OnchainSendV1 { note, .. }) => note,
            Self::OnchainReceive(OnchainReceiveV1 { note, .. }) => note,
            Self::InboundInvoice(InboundInvoicePaymentV1 { note, .. }) => note,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                note,
                ..
            }) => note,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                note,
                ..
            }) => note,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                note, ..
            }) => note,
            Self::OutboundOffer(OutboundOfferPaymentV1 { note, .. }) => note,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                note,
                ..
            }) => note,
        }
        .as_ref()
        .map(|s| s.as_str())
    }

    /// Set the payment note to a new value.
    pub fn set_note(&mut self, note: Option<String>) {
        let mut_ref_note: &mut Option<String> = match self {
            Self::OnchainSend(OnchainSendV1 { note, .. }) => note,
            Self::OnchainReceive(OnchainReceiveV1 { note, .. }) => note,
            Self::InboundInvoice(InboundInvoicePaymentV1 { note, .. }) => note,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                note,
                ..
            }) => note,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                note,
                ..
            }) => note,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                note, ..
            }) => note,
            Self::OutboundOffer(OutboundOfferPaymentV1 { note, .. }) => note,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                note,
                ..
            }) => note,
        };

        *mut_ref_note = note;
    }

    /// When this payment was created.
    pub fn created_at(&self) -> TimestampMs {
        match self {
            Self::OnchainSend(OnchainSendV1 { created_at, .. }) => *created_at,
            Self::OnchainReceive(OnchainReceiveV1 { created_at, .. }) =>
                *created_at,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                created_at,
                ..
            }) => *created_at,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                created_at,
                ..
            }) => *created_at,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundOffer(OutboundOfferPaymentV1 {
                created_at, ..
            }) => *created_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                created_at,
                ..
            }) => *created_at,
        }
    }

    /// When this payment was completed or failed.
    pub fn finalized_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(OnchainSendV1 { finalized_at, .. }) =>
                *finalized_at,
            Self::OnchainReceive(OnchainReceiveV1 { finalized_at, .. }) =>
                *finalized_at,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundOffer(OutboundOfferPaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV1 {
                finalized_at,
                ..
            }) => *finalized_at,
        }
    }

    /// Assert invariants on the current `Payment` state when
    /// `cfg!(debug_assertions)` is enabled. This is a no-op in production.
    pub(crate) fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // Payments should have a finalized_at() iff it has finalized.
        use PaymentStatus::*;
        match self.status() {
            Pending => assert!(self.finalized_at().is_none()),
            Completed | Failed => assert!(self.finalized_at().is_some()),
        }
    }
}

#[cfg(test)]
mod test {
    use std::{fs, path::Path};

    use common::{
        aes::AesMasterKey,
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::{
        arbitrary::any, prelude::Strategy, prop_assert_eq, proptest,
        test_runner::Config,
    };

    use super::*;
    use crate::payments;

    // Generate serialized `BasicPaymentV1` sample json data:
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
                any::<OnchainSendV1>()
                    .prop_map(PaymentV1::OnchainSend)
                    .boxed(),
            ),
            (
                "OnchainReceive",
                any::<OnchainReceiveV1>()
                    .prop_map(PaymentV1::OnchainReceive)
                    .boxed(),
            ),
            (
                "InboundInvoice",
                any::<InboundInvoicePaymentV1>()
                    .prop_map(PaymentV1::InboundInvoice)
                    .boxed(),
            ),
            (
                "InboundOfferReusable",
                any::<InboundOfferReusablePaymentV1>()
                    .prop_map(PaymentV1::InboundOfferReusable)
                    .boxed(),
            ),
            (
                "InboundSpontaneous",
                any::<InboundSpontaneousPaymentV1>()
                    .prop_map(PaymentV1::InboundSpontaneous)
                    .boxed(),
            ),
            (
                "OutboundInvoice",
                any::<OutboundInvoicePaymentV1>()
                    .prop_map(PaymentV1::OutboundInvoice)
                    .boxed(),
            ),
            (
                "OutboundOfferPayment",
                any::<OutboundOfferPaymentV1>()
                    .prop_map(PaymentV1::OutboundOffer)
                    .boxed(),
            ),
            (
                "OutboundSpontaneous",
                any::<OutboundSpontaneousPaymentV1>()
                    .prop_map(PaymentV1::OutboundSpontaneous)
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

                // serialize app BasicPaymentV1
                let value = BasicPaymentV1::from(value);
                let json = serde_json::to_string(&value).unwrap();
                println!("{json}");
            }
        }
    }

    #[test]
    fn top_level_payment_serde_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<PaymentV1>();
    }

    #[test]
    fn low_level_payments_serde_roundtrips() {
        use roundtrip::json_value_custom;
        let config = Config::with_cases(16);
        json_value_custom(any::<OnchainSendV1>(), config.clone());
        json_value_custom(any::<OnchainReceiveV1>(), config.clone());
        // TODO(max): Add SpliceIn
        // TODO(max): Add SpliceOut
        json_value_custom(any::<InboundInvoicePaymentV1>(), config.clone());
        json_value_custom(
            any::<InboundOfferReusablePaymentV1>(),
            config.clone(),
        );
        json_value_custom(any::<InboundSpontaneousPaymentV1>(), config.clone());
        json_value_custom(any::<OutboundInvoicePaymentV1>(), config.clone());
        json_value_custom(any::<OutboundOfferPaymentV1>(), config.clone());
        json_value_custom(any::<OutboundSpontaneousPaymentV1>(), config);
    }

    #[test]
    fn payment_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            p1 in any::<PaymentV1>(),
            updated_at in any::<TimestampMs>(),
        )| {
            let encrypted = payments::encrypt(
                &mut rng, &vfs_master_key, &p1, updated_at
            );
            let p2 = payments::decrypt(&vfs_master_key, encrypted.data).unwrap();
            prop_assert_eq!(p1, p2);
        })
    }

    #[test]
    fn payment_id_equivalence() {
        let cfg = Config::with_cases(100);

        proptest!(cfg, |(payment: PaymentV1)| {
            let id = match &payment {
                PaymentV1::OnchainSend(x) => x.id(),
                PaymentV1::OnchainReceive(x) => x.id(),
                PaymentV1::InboundInvoice(x) => x.id(),
                PaymentV1::InboundOfferReusable(x) => x.id(),
                PaymentV1::InboundSpontaneous(x) => x.id(),
                PaymentV1::OutboundInvoice(x) => x.id(),
                PaymentV1::OutboundOffer(x) => x.id(),
                PaymentV1::OutboundSpontaneous(x) => x.id(),
            };
            prop_assert_eq!(id, payment.id());
        });
    }

    /// Dumps a JSON array of `Payment`s using the proptest strategy.
    /// Generates N of each payment sub-type to ensure even coverage.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored take_payments_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn take_payments_snapshot() {
        const COUNT: usize = 5;
        let seed = 20250316; // Base seed for all variants
        let mut rng = FastRng::from_u64(seed);
        let mut payments = Vec::new();

        // Generate COUNT of each payment type for even coverage
        payments.extend(
            arbitrary::gen_value_iter(&mut rng, any::<OnchainSendV1>())
                .take(COUNT)
                .map(PaymentV1::OnchainSend),
        );
        payments.extend(
            arbitrary::gen_value_iter(&mut rng, any::<OnchainReceiveV1>())
                .take(COUNT)
                .map(PaymentV1::OnchainReceive),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundInvoicePaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::InboundInvoice),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundOfferReusablePaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::InboundOfferReusable),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundSpontaneousPaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::InboundSpontaneous),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundInvoicePaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::OutboundInvoice),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundOfferPaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::OutboundOffer),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundSpontaneousPaymentV1>(),
            )
            .take(COUNT)
            .map(PaymentV1::OutboundSpontaneous),
        );

        println!("---");
        println!("{}", serde_json::to_string_pretty(&payments).unwrap());
        println!("---");
    }

    #[test]
    fn test_payment_snapshots() {
        let snapshot_path = Path::new("data/payment-snapshot.v1.json");
        let snapshot = fs::read_to_string(snapshot_path)
            .expect("Failed to read payment snapshot");
        serde_json::from_str::<Vec<PaymentV1>>(&snapshot)
            .expect("Failed to deserialize payment snapshot");
    }
}
