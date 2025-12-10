use std::sync::Arc;
#[cfg(test)]
use std::{collections::HashSet, num::NonZeroU64};

use anyhow::Context;
#[cfg(test)]
use common::ln::priority::ConfirmationPriority;
use common::{
    ln::{amount::Amount, hashes::LxTxid},
    time::TimestampMs,
};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        BasicPaymentV1, LxOfferId, LxPaymentId, PaymentCreatedIndex,
        PaymentDirection, PaymentRail, PaymentStatus,
    },
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::payments::{
    PaymentV2, PaymentWithMetadata,
    inbound::{
        InboundInvoicePaymentV2, InboundOfferReusablePaymentV2,
        InboundSpontaneousPaymentV2,
    },
    onchain::{OnchainReceiveV2, OnchainSendV2},
    outbound::{
        OutboundInvoicePaymentV2, OutboundOfferPaymentV2,
        OutboundSpontaneousPaymentV2,
    },
    v1::{
        inbound::{
            InboundInvoicePaymentV1, InboundOfferReusablePaymentV1,
            InboundSpontaneousPaymentV1,
        },
        onchain::{OnchainReceiveV1, OnchainSendV1},
        outbound::{
            OutboundInvoicePaymentV1, OutboundOfferPaymentV1,
            OutboundSpontaneousPaymentV1,
        },
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

// --- Payment subtype -> top-level Payment type --- //

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

// --- Conversion to/from PaymentWithMetadata --- //

impl From<PaymentV1> for PaymentWithMetadata {
    fn from(payment_v1: PaymentV1) -> Self {
        match payment_v1 {
            PaymentV1::OnchainSend(p) =>
                PaymentWithMetadata::<OnchainSendV2>::from(p).into_enum(),
            PaymentV1::OnchainReceive(p) =>
                PaymentWithMetadata::<OnchainReceiveV2>::from(p).into_enum(),
            PaymentV1::InboundInvoice(p) =>
                PaymentWithMetadata::<InboundInvoicePaymentV2>::from(p)
                    .into_enum(),
            PaymentV1::InboundOfferReusable(p) =>
                PaymentWithMetadata::<InboundOfferReusablePaymentV2>::from(p)
                    .into_enum(),
            PaymentV1::InboundSpontaneous(p) =>
                PaymentWithMetadata::<InboundSpontaneousPaymentV2>::from(p)
                    .into_enum(),
            PaymentV1::OutboundInvoice(p) =>
                PaymentWithMetadata::<OutboundInvoicePaymentV2>::from(p)
                    .into_enum(),
            PaymentV1::OutboundOffer(p) =>
                PaymentWithMetadata::<OutboundOfferPaymentV2>::from(p)
                    .into_enum(),
            PaymentV1::OutboundSpontaneous(p) =>
                PaymentWithMetadata::<OutboundSpontaneousPaymentV2>::from(p)
                    .into_enum(),
        }
    }
}

impl TryFrom<PaymentWithMetadata> for PaymentV1 {
    type Error = anyhow::Error;

    fn try_from(pwm: PaymentWithMetadata) -> Result<Self, Self::Error> {
        let v1 = match pwm.payment {
            PaymentV2::OnchainSend(osv2) => {
                let oswm = PaymentWithMetadata::<OnchainSendV2> {
                    payment: osv2,
                    metadata: pwm.metadata,
                };
                let osv1 = OnchainSendV1::try_from(oswm)
                    .context("OnchainSend conversion")?;
                PaymentV1::OnchainSend(osv1)
            }
            PaymentV2::OnchainReceive(osv2) => {
                let orwm = PaymentWithMetadata::<OnchainReceiveV2> {
                    payment: osv2,
                    metadata: pwm.metadata,
                };
                let orv1 = OnchainReceiveV1::try_from(orwm)
                    .context("OnchainReceive conversion")?;
                PaymentV1::OnchainReceive(orv1)
            }
            PaymentV2::InboundInvoice(iipv2) => {
                let iipwm = PaymentWithMetadata::<InboundInvoicePaymentV2> {
                    payment: iipv2,
                    metadata: pwm.metadata,
                };
                let iipv1 = InboundInvoicePaymentV1::try_from(iipwm)
                    .context("InboundInvoice conversion")?;
                PaymentV1::InboundInvoice(iipv1)
            }
            PaymentV2::InboundOfferReusable(iorpv2) => {
                let iorpwm =
                    PaymentWithMetadata::<InboundOfferReusablePaymentV2> {
                        payment: iorpv2,
                        metadata: pwm.metadata,
                    };
                let iorpv1 = InboundOfferReusablePaymentV1::try_from(iorpwm)
                    .context("InboundOfferReusable conversion")?;
                PaymentV1::InboundOfferReusable(iorpv1)
            }
            PaymentV2::InboundSpontaneous(ispv2) => {
                let ispwm = PaymentWithMetadata::<InboundSpontaneousPaymentV2> {
                    payment: ispv2,
                    metadata: pwm.metadata,
                };
                let ispv1 = InboundSpontaneousPaymentV1::try_from(ispwm)
                    .context("InboundSpontaneous conversion")?;
                PaymentV1::InboundSpontaneous(ispv1)
            }
            PaymentV2::OutboundInvoice(oipv2) => {
                let oipwm = PaymentWithMetadata::<OutboundInvoicePaymentV2> {
                    payment: oipv2,
                    metadata: pwm.metadata,
                };
                let oipv1 = OutboundInvoicePaymentV1::try_from(oipwm)
                    .context("OutboundInvoice conversion")?;
                PaymentV1::OutboundInvoice(oipv1)
            }
            PaymentV2::OutboundOffer(oopv2) => {
                let oopwm = PaymentWithMetadata::<OutboundOfferPaymentV2> {
                    payment: oopv2,
                    metadata: pwm.metadata,
                };
                let oopv1 = OutboundOfferPaymentV1::try_from(oopwm)
                    .context("OutboundOffer conversion")?;
                PaymentV1::OutboundOffer(oopv1)
            }
            PaymentV2::OutboundSpontaneous(p) => {
                let ospwm = PaymentWithMetadata {
                    payment: p,
                    metadata: pwm.metadata,
                };
                let ospv1 = OutboundSpontaneousPaymentV1::try_from(ospwm)
                    .context("OutboundSpontaneous conversion")?;
                PaymentV1::OutboundSpontaneous(ospv1)
            }
        };

        Ok(v1)
    }
}

// --- Payment -> BasicPaymentV1 --- //

impl From<PaymentV1> for BasicPaymentV1 {
    fn from(p: PaymentV1) -> Self {
        Self {
            index: p.index(),
            rail: p.rail(),
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
    /// NOTE: Keep this around to ensure the new `into_basic_payment` impl
    /// remains consistent with the older, known-correct version (see proptest
    /// below), until all logic has been migrated to the v2 types with proptests
    /// passing.
    #[cfg(test)]
    pub fn into_basic_payment(
        self,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> lexe_api::types::payments::BasicPaymentV2 {
        use lexe_api::types::payments::PaymentKind;

        let kind = match self.rail() {
            PaymentRail::Onchain => PaymentKind::Onchain,
            PaymentRail::Invoice => PaymentKind::Invoice,
            PaymentRail::Offer => PaymentKind::Offer,
            PaymentRail::Spontaneous => PaymentKind::Spontaneous,
            // All V1 variants handled above
            PaymentRail::Unknown(_) => unreachable!(),
            // V1 payments don't have these kinds
            PaymentRail::WaivedFee => unreachable!(),
        };

        lexe_api::types::payments::BasicPaymentV2 {
            id: self.id(),
            related_ids: HashSet::new(),
            kind,
            direction: self.direction(),
            offer_id: self.offer_id(),
            txid: self.txid(),
            amount: self.amount(),
            fee: self.fees(),
            // channel_fee: self.channel_fee(),
            status: self.status(),
            status_str: self.status_str().to_owned(),
            address: None,
            invoice: self.invoice(),
            offer: self.offer(),
            tx: self.tx(),
            note: self.note().map(|s| s.to_owned()),
            payer_name: self.payer_name().map(|s| s.to_owned()),
            payer_note: self.payer_note().map(|s| s.to_owned()),
            priority: self.priority(),
            quantity: self.quantity(),
            replacement_txid: self.replacement(),
            expires_at: self.expires_at(),
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
    pub fn rail(&self) -> PaymentRail {
        match self {
            Self::OnchainSend(_) => PaymentRail::Onchain,
            Self::OnchainReceive(_) => PaymentRail::Onchain,
            Self::InboundInvoice(_) => PaymentRail::Invoice,
            Self::InboundOfferReusable(_) => PaymentRail::Offer,
            Self::InboundSpontaneous(_) => PaymentRail::Spontaneous,
            Self::OutboundInvoice(_) => PaymentRail::Invoice,
            Self::OutboundOffer(_) => PaymentRail::Offer,
            Self::OutboundSpontaneous(_) => PaymentRail::Spontaneous,
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

    /// Returns the BOLT11 invoice corresponding to this payment, if any.
    pub fn invoice(&self) -> Option<Arc<LxInvoice>> {
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
    pub fn offer(&self) -> Option<Arc<LxOffer>> {
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

    /// Returns the expiry time for invoice or offer payments.
    #[cfg(test)]
    pub fn expires_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                invoice, ..
            }) => invoice.expires_at().ok(),
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(OutboundInvoicePaymentV1 {
                invoice, ..
            }) => invoice.expires_at().ok(),
            Self::OutboundOffer(OutboundOfferPaymentV1 { offer, .. }) =>
                offer.expires_at(),
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

    /// Returns the on-chain transaction if this is an onchain payment.
    #[cfg(test)]
    pub fn tx(&self) -> Option<Arc<bitcoin::Transaction>> {
        match self {
            Self::OnchainSend(OnchainSendV1 { tx, .. }) => Some(tx.clone()),
            Self::OnchainReceive(OnchainReceiveV1 { tx, .. }) =>
                Some(tx.clone()),
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
            }) => onchain_fees.unwrap_or(Amount::ZERO),
            // TODO(phlip9): impl LSP skimming to charge receiver for fees
            Self::InboundOfferReusable(_) => Amount::ZERO,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                onchain_fees,
                ..
            }) => onchain_fees.unwrap_or(Amount::ZERO),
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

    /* TODO(max): Implement JIT channel fees
    /// The portion of the skimmed amount that was used to cover the on-chain
    /// fees incurred by a JIT channel opened to receive this payment.
    pub fn channel_fee(&self) -> Option<Amount> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(InboundInvoicePaymentV1 {
                onchain_fees,
                ..
            }) => *onchain_fees,
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV1 {
                onchain_fees,
                ..
            }) => *onchain_fees,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(_) => None,
            Self::OutboundSpontaneous(_) => None,
        }
    }
    */

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

    /// Get the confirmation priority for onchain send payments.
    #[cfg(test)]
    pub fn priority(&self) -> Option<ConfirmationPriority> {
        match self {
            Self::OnchainSend(OnchainSendV1 { priority, .. }) =>
                Some(*priority),
            _ => None,
        }
    }

    /// Get the quantity (number of items purchased) for offer payments.
    #[cfg(test)]
    pub fn quantity(&self) -> Option<NonZeroU64> {
        match self {
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                quantity,
                ..
            }) => *quantity,
            Self::OutboundOffer(OutboundOfferPaymentV1 {
                quantity, ..
            }) => *quantity,
            _ => None,
        }
    }

    /// Get the payer-provided note for inbound offer reusable payments.
    #[cfg(test)]
    pub fn payer_note(&self) -> Option<&str> {
        match self {
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                payer_note,
                ..
            }) => payer_note,
            _ => &None,
        }
        .as_ref()
        .map(|s| s.as_str())
    }

    /// Get the payer name for inbound offer reusable payments.
    #[cfg(test)]
    pub fn payer_name(&self) -> Option<&str> {
        match self {
            Self::InboundOfferReusable(InboundOfferReusablePaymentV1 {
                payer_name,
                ..
            }) => payer_name,
            _ => &None,
        }
        .as_ref()
        .map(|s| s.as_str())
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
        arbitrary::any, prop_assert_eq, proptest, test_runner::Config,
    };

    use super::*;
    use crate::payments;

    /// During migration, we need to maintain the invariant that
    /// `PaymentV1 -> PaymentWithMetadata -> PaymentV1` is lossless, as we are
    /// moving all *logic* from `PaymentV1` to `PaymentV2`, but will still use
    /// `PaymentV1` for serialization until all logic has been migrated.
    #[test]
    fn payment_v1_v2_roundtrip_proptest() {
        proptest!(|(p1 in any::<PaymentV1>())| {
            let pwm = PaymentWithMetadata::from(p1.clone());
            let p2 = PaymentV1::try_from(pwm).unwrap();
            prop_assert_eq!(p1, p2);
        })
    }

    /// Tests that
    ///
    /// - `PaymentV1` -> `BasicPaymentV2`
    /// - `PaymentV1` -> `PaymentWithMetadata` -> `BasicPaymentV2`
    ///
    /// are equivalent.
    #[test]
    fn v1_v2_into_basic_payment_proptest() {
        proptest!(|(
            payment in any::<PaymentV1>(),
            created_at in any::<TimestampMs>(),
            updated_at in any::<TimestampMs>(),
        )| {
            let pwm = PaymentWithMetadata::from(payment.clone());
            let basic_direct =
                payment.into_basic_payment(created_at, updated_at);
            let basic_via_pwm = pwm.into_basic_payment(created_at, updated_at);
            prop_assert_eq!(basic_direct, basic_via_pwm);
        })
    }

    #[test]
    fn v1_subtypes_serde_roundtrips() {
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
    fn payment_v1_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            p1_v1 in any::<PaymentV1>(),
            now in any::<TimestampMs>(),
        )| {
            let pwm = PaymentWithMetadata::from(p1_v1.clone());
            let p1 = pwm.payment.clone();

            let created_at = p1.created_at().unwrap_or(now);
            let updated_at = now;

            let encrypted = payments::encrypt_v1(
                &mut rng,
                &vfs_master_key,
                &pwm,
                created_at,
                updated_at,
            )
            .unwrap();
            let p2 = payments::decrypt_v1(&vfs_master_key, encrypted.data)
                .map(|pwm| pwm.payment)
                .unwrap();
            prop_assert_eq!(p1, p2);
        })
    }

    /// Dumps a JSON array of `Payment`s using the proptest strategy.
    /// Generates N of each payment sub-type to ensure even coverage.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored take_payments_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn take_payments_v1_snapshot() {
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
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn payment_v1_snapshot_test() {
        let snapshot_path = Path::new("data/payment-snapshot.v1.json");
        let snapshot = fs::read_to_string(snapshot_path)
            .expect("Failed to read payment snapshot");
        serde_json::from_str::<Vec<PaymentV1>>(&snapshot)
            .expect("Failed to deserialize payment snapshot");
    }
}
