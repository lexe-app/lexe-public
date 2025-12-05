//! Lexe payments types and logic.
//!
//! This module is the 'complex' counterpart to the simpler types exposed in
//! [`lexe_api::types::payments`].

use std::{borrow::Cow, collections::HashSet, num::NonZeroU64, sync::Arc};

use anyhow::Context;
use bitcoin::address::NetworkUnchecked;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    aes::AesMasterKey,
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    rng::Crng,
    time::TimestampMs,
};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        BasicPaymentV2, DbPaymentV2, LxOfferId, LxPaymentId, PaymentClass,
        PaymentDirection, PaymentKind, PaymentStatus,
    },
};
use lexe_std::const_assert_mem_size;
#[cfg(test)]
use proptest::{option, prelude::Just};
#[cfg(test)]
use proptest_derive::Arbitrary;
#[cfg(doc)]
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde::{Deserialize, Serialize};

use crate::payments::{
    inbound::{
        InboundInvoicePaymentStatus, InboundInvoicePaymentV2,
        InboundOfferReusablePaymentStatus, InboundOfferReusablePaymentV2,
        InboundSpontaneousPaymentStatus, InboundSpontaneousPaymentV2,
    },
    onchain::{
        OnchainReceiveStatus, OnchainReceiveV2, OnchainSendStatus,
        OnchainSendV2,
    },
    outbound::{
        OutboundInvoicePaymentStatus, OutboundInvoicePaymentV2,
        OutboundOfferPaymentStatus, OutboundOfferPaymentV2,
        OutboundSpontaneousPaymentStatus, OutboundSpontaneousPaymentV2,
    },
    v1::PaymentV1,
};

/// Inbound Lightning payments.
pub mod inbound;
/// `PaymentsManager`.
pub mod manager;
/// On-chain payment types and state machines.
pub mod onchain;
/// Outbound Lightning payments.
pub mod outbound;
/// `PaymentV1` and sub-types.
pub mod v1;

// --- Top-level payment types --- //

/// Associates a payment with its payment metadata.
/// Defaults to the top-level payment type [`PaymentV2`], but can be used for
/// any payment subtype, e.g. [`OnchainSendV2`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentWithMetadata<P = PaymentV2> {
    pub payment: P,
    pub metadata: PaymentMetadata,
}

/// The primary `Payment` enum which abstracts over all types of payments,
/// including both onchain and off-chain (Lightning) payments.
///
/// Each variant in `Payment` implements a state machine that ingests events
/// from [`PaymentsManager`] to transition between states in that payment type's
/// lifecycle. For example, we create an [`OnchainSendV2`] payment in its
/// initial state, `Created`. After we successfully broadcast the tx, the
/// payment transitions to `Broadcasted`. Once the tx confirms, the payment
/// transitions to `PartiallyConfirmed`, then `FullyConfirmed` with 6+ confs.
///
/// ### Should data go in a [`PaymentV2`] subtype or in `PaymentMetadata`?
///
/// NOTE: The core state machines in the [`PaymentV2`] subtypes are extremely
/// 'sharp' and have strict requirements with regards to idempotency,
/// consistency, timestamp monotonicity, serialization compatibility, and more.
/// Making any changes to these state machines requires careful reasoning which
/// considers the entire tower of logic:
///
/// - `background_processor`
/// - `EventHandler` and event persistence
/// - `PaymentsManager` with locking and persistence
/// - `PaymentsData`, with finalized payment cache
/// - `PaymentV2`
/// - `PaymentV2` subtypes: `OnchainSendV2`, `OutboundInvoicePaymentV1`, etc.
///
/// THEREFORE, it is in your interest to minimize the amount of data stored in
/// the [`PaymentV2`] subtypes. The general rule of thumb is that [`PaymentV2`]
/// subtypes should only contain information necessary to validate correctness
/// and make progress in its state machine (e.g. payment hashes/secrets, txids),
/// or which needs to be well-structured for financial accounting,
/// categorization, and reconciliation (e.g. amounts, fees, class). Everything
/// else, especially large blobs of data, should go in [`PaymentMetadata`].
///
/// Another way to think of it:
///
/// - [`PaymentV2`] subtypes can be thought of as the payment 'rails' -
///   responsible for moving value from one place to another.
/// - [`PaymentMetadata`] contains 'metadata' - enriches the core payment types
///   with semantic data that is of interest to users and applications, such as
///   descriptions/notes, invoices/offers, referer IDs, initiating clients, fee
///   rates, interest rate APYs, etc.
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
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
// TODO(max): This should derive Serialize and Deserialize, but we hold off for
// now as we don't want to accidentally serialize using this type while we're
// locking down the serialization format.
// TODO(max): Figure out how `class` should be represented before committing to
// the PaymentV2 serialization scheme. Perhaps the payment should be a tagged
// enum, like `struct PaymentV2 { payment: PaymentEnum, class: PaymentClass }`?
// TODO(max): Gen and inspect sample data before committing to serialization
#[cfg_attr(test, derive(Serialize, Deserialize))]
pub enum PaymentV2 {
    OnchainSend(OnchainSendV2),
    OnchainReceive(OnchainReceiveV2),
    // TODO(max): Implement SpliceIn
    // TODO(max): Implement SpliceOut
    InboundInvoice(InboundInvoicePaymentV2),
    // TODO(phlip9): InboundOffer (single-use)
    // Added in `node-v0.7.8`
    InboundOfferReusable(InboundOfferReusablePaymentV2),
    InboundSpontaneous(InboundSpontaneousPaymentV2),
    OutboundInvoice(OutboundInvoicePaymentV2),
    // Added in `node-v0.7.8`
    OutboundOffer(OutboundOfferPaymentV2),
    OutboundSpontaneous(OutboundSpontaneousPaymentV2),
}

// Debug the size_of `PaymentV2`
const_assert_mem_size!(PaymentV2, 240);

/// Optional payment metadata associated with a [`PaymentV2`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
// TODO(max): This should derive Serialize and Deserialize, but we hold off for
// now as we don't want to accidentally serialize using this type while we're
// still migrating all logic.
#[cfg_attr(test, derive(Serialize, Deserialize))]
pub struct PaymentMetadata {
    // --- Identifier and basic info fields --- //
    // -
    /// Payment identifier; globally unique from the user's perspective.
    pub id: LxPaymentId,

    /// The ids of payments related to this payment.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_hashset::<LxPaymentId>()")
    )]
    pub related_ids: HashSet<LxPaymentId>,

    // --- Payment methods --- //
    // -
    /// (On-chain send only) The address that we're sending to.
    #[cfg_attr(
        test,
        proptest(
            strategy = "arbitrary::any_option_arc_mainnet_addr_unchecked()"
        )
    )]
    pub address: Option<Arc<bitcoin::Address<NetworkUnchecked>>>,

    /// The BOLT11 invoice corresponding to this payment, if any.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary_helpers::any_option_arc_invoice()")
    )]
    pub invoice: Option<Arc<LxInvoice>>,

    /// The BOLT12 offer associated with this payment, if any.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary_helpers::any_option_arc_offer()")
    )]
    pub offer: Option<Arc<LxOffer>>,

    // --- Notes and sender/receiver identifiers --- //
    // -
    /// The payment note, private to the user.
    // Suppress useless unicode gibberish in tests.
    #[cfg_attr(
        test,
        proptest(strategy = "option::of(Just(String::from(\"note\")))")
    )]
    pub note: Option<String>,

    /// (Inbound offer reusable only)
    /// The payer's self-reported human-readable name.
    #[cfg_attr(
        test,
        proptest(strategy = "option::of(Just(String::from(\"payer name\")))")
    )]
    pub payer_name: Option<String>,

    /// (Offers only) A payer-provided note for this payment.
    /// LDK truncates this to PAYER_NOTE_LIMIT bytes (512 B as of 2025-04-22).
    #[cfg_attr(
        test,
        proptest(strategy = "option::of(Just(String::from(\"payer note\")))")
    )]
    pub payer_note: Option<String>,

    // --- Other --- //
    // -
    /// (On-chain send only) The confirmation priority used for this payment.
    pub priority: Option<ConfirmationPriority>,

    /// (Inbound offer reusable only) The number of items the payer bought.
    pub quantity: Option<NonZeroU64>,

    /// (Onchain payments only) The txid of the replacement tx, if one exists.
    pub replacement_txid: Option<LxTxid>,
}

// Debug the size_of `PaymentMetadata`
const_assert_mem_size!(PaymentMetadata, 224);

/// An update to a [`PaymentMetadata`].
#[must_use]
#[derive(Debug, Default, PartialEq)]
pub(crate) struct PaymentMetadataUpdate {
    // --- Identifier and basic info fields --- //
    // -
    /// The ids of payments newly associated with this payment.
    pub new_related_ids: HashSet<LxPaymentId>,

    // TODO(max): Can keep this commented until we're sure we actually need it
    // /// The ids of payments no longer associated with this payment.
    // pub removed_related_ids: HashSet<LxPaymentId>,

    // --- Payment methods --- //
    pub address: Option<Option<Arc<bitcoin::Address<NetworkUnchecked>>>>,

    pub invoice: Option<Option<Arc<LxInvoice>>>,

    pub offer: Option<Option<Arc<LxOffer>>>,

    // --- Notes and sender/receiver identifiers --- //
    pub note: Option<Option<String>>,

    pub payer_name: Option<Option<String>>,

    pub payer_note: Option<Option<String>>,

    // --- Other --- //
    pub priority: Option<Option<ConfirmationPriority>>,

    pub quantity: Option<Option<NonZeroU64>>,

    pub replacement_txid: Option<Option<LxTxid>>,
}

// --- Encryption --- //

/// Serializes a given payment to JSON and encrypts the payment under the given
/// [`AesMasterKey`], returning the [`DbPaymentV2`] which can be persisted.
// TODO(max): Make infallible again once we use PaymentV2
pub fn encrypt_v1(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    pwm: &PaymentWithMetadata,
    created_at: TimestampMs,
    updated_at: TimestampMs,
) -> anyhow::Result<DbPaymentV2> {
    // Serialize the payment as JSON bytes.
    let aad = &[];
    let data_size_hint = None;
    // NOTE: This serializes using v1
    let payment_v1 = PaymentV1::try_from(pwm.clone())
        .context("Failed to convert payment to v1")?;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|mut_vec_u8| {
        serde_json::to_writer(mut_vec_u8, &payment_v1)
            .expect("Payment serialization always succeeds")
    };

    // Encrypt.
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    Ok(DbPaymentV2 {
        id: pwm.payment.id().to_string(),
        class: Some(Cow::Borrowed(pwm.payment.class().as_str())),
        direction: Some(Cow::Borrowed(pwm.payment.direction().as_str())),
        amount: pwm.payment.amount(),
        fee: Some(pwm.payment.fee()),
        status: Cow::Borrowed(pwm.payment.status().as_str()),
        data,
        version: 1,
        created_at: created_at.to_i64(),
        updated_at: updated_at.to_i64(),
    })
}

/// Given a [`DbPaymentV2::data`] (ciphertext), attempts to decrypt using the
/// given [`AesMasterKey`], returning the deserialized [`PaymentV2`].
pub fn decrypt_v1(
    vfs_master_key: &AesMasterKey,
    data: Vec<u8>,
) -> anyhow::Result<PaymentWithMetadata> {
    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, data)
        .context("Could not decrypt Payment")?;

    // NOTE: This deserializes using v1
    serde_json::from_slice::<PaymentV1>(plaintext_bytes.as_slice())
        .map(PaymentWithMetadata::from)
        .context("Could not deserialize Payment")
}

// TODO(max): Add `encrypt_payment` which uses v2 types
// TODO(max): Add `encrypt_metadata which uses v2 types
// TODO(max): Add `decrypt_payment which uses v2 types
// TODO(max): Add `decrypt_metadata which uses v2 types

// --- Payment subtype -> top-level Payment type --- //

impl From<OnchainSendV2> for PaymentV2 {
    fn from(p: OnchainSendV2) -> Self {
        Self::OnchainSend(p)
    }
}
impl From<OnchainReceiveV2> for PaymentV2 {
    fn from(p: OnchainReceiveV2) -> Self {
        Self::OnchainReceive(p)
    }
}
impl From<InboundInvoicePaymentV2> for PaymentV2 {
    fn from(p: InboundInvoicePaymentV2) -> Self {
        Self::InboundInvoice(p)
    }
}
impl From<InboundOfferReusablePaymentV2> for PaymentV2 {
    fn from(p: InboundOfferReusablePaymentV2) -> Self {
        Self::InboundOfferReusable(p)
    }
}
impl From<InboundSpontaneousPaymentV2> for PaymentV2 {
    fn from(p: InboundSpontaneousPaymentV2) -> Self {
        Self::InboundSpontaneous(p)
    }
}
impl From<OutboundInvoicePaymentV2> for PaymentV2 {
    fn from(p: OutboundInvoicePaymentV2) -> Self {
        Self::OutboundInvoice(p)
    }
}
impl From<OutboundOfferPaymentV2> for PaymentV2 {
    fn from(p: OutboundOfferPaymentV2) -> Self {
        Self::OutboundOffer(p)
    }
}
impl From<OutboundSpontaneousPaymentV2> for PaymentV2 {
    fn from(p: OutboundSpontaneousPaymentV2) -> Self {
        Self::OutboundSpontaneous(p)
    }
}

// --- impl PaymentWithMetadata --- //

impl<P: Into<PaymentV2>> PaymentWithMetadata<P> {
    /// Maps the payment sub-type to the `PaymentV2` enum, e.g.
    /// `PaymentWithMetadata<OnchainSendV2>` -> `PaymentWithMetadata<PaymentV2>`
    pub fn into_enum(self) -> PaymentWithMetadata {
        PaymentWithMetadata {
            payment: self.payment.into(),
            metadata: self.metadata,
        }
    }
}

impl PaymentWithMetadata<PaymentV2> {
    // Can't impl BasicPaymentV2::from_payment bc we don't want to move
    // `Payment` into `lexe-api-core`.
    pub fn into_basic_payment(
        self,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> BasicPaymentV2 {
        let id = self.payment.id();
        let txid = self.payment.txid();
        let offer_id = self.payment.offer_id();
        let kind = self.payment.kind();
        let class = self.payment.class();
        let direction = self.payment.direction();
        let status = self.payment.status();
        let status_str = self.payment.status_str().to_owned();
        let amount = self.payment.amount();
        let fee = self.payment.fee();
        // let channel_fee = self.payment.channel_fee();
        let tx = self.payment.tx();
        let expires_at = self.payment.expires_at();
        let finalized_at = self.payment.finalized_at();

        let related_ids = self.metadata.related_ids;
        let address = self.metadata.address;
        let invoice = self.metadata.invoice;
        let offer = self.metadata.offer;
        let note = self.metadata.note;
        let payer_name = self.metadata.payer_name;
        let payer_note = self.metadata.payer_note;
        let priority = self.metadata.priority;
        let quantity = self.metadata.quantity;
        let replacement_txid = self.metadata.replacement_txid;

        BasicPaymentV2 {
            id,
            related_ids,
            kind,
            class,
            direction,
            offer_id,
            txid,
            amount,
            fee,
            // channel_fee,
            status,
            status_str,
            address,
            invoice,
            offer,
            tx,
            note,
            payer_name,
            payer_note,
            priority,
            quantity,
            replacement_txid,
            expires_at,
            finalized_at,
            created_at,
            updated_at,
        }
    }
}

// --- impl PaymentMetadata / PaymentMetadataUpdate --- //

impl PaymentMetadata {
    /// Construct an empty `PaymentMetadata`.
    pub fn empty(id: LxPaymentId) -> Self {
        Self {
            id,
            related_ids: HashSet::new(),
            address: None,
            invoice: None,
            offer: None,
            note: None,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
        }
    }

    /// Whether all fields other than the required `id` are empty, meaning
    /// this does not need to be persisted; it can be safely discarded.
    pub fn is_empty(&self) -> bool {
        // We intentionally destructure here to ensure we get a compilation
        // error whenever we add another field
        let Self {
            id: _,
            related_ids,
            address,
            invoice,
            offer,
            note,
            payer_name,
            payer_note,
            priority,
            quantity,
            replacement_txid,
        } = self;

        related_ids.is_empty()
            && address.is_none()
            && invoice.is_none()
            && offer.is_none()
            && note.is_none()
            && payer_name.is_none()
            && payer_note.is_none()
            && priority.is_none()
            && quantity.is_none()
            && replacement_txid.is_none()
    }

    /// Applies a metadata update to this [`PaymentMetadata`].
    pub(crate) fn apply_update(
        mut self,
        update: PaymentMetadataUpdate,
    ) -> Self {
        // We intentionally destructure here to ensure we get a compilation
        // error whenever we add another field
        let PaymentMetadataUpdate {
            new_related_ids,
            address,
            invoice,
            offer,
            note,
            payer_name,
            payer_note,
            priority,
            quantity,
            replacement_txid,
        } = update;

        self.related_ids.extend(new_related_ids);
        self.address = address.unwrap_or(self.address);
        self.invoice = invoice.unwrap_or(self.invoice);
        self.offer = offer.unwrap_or(self.offer);
        self.note = note.unwrap_or(self.note);
        self.payer_name = payer_name.unwrap_or(self.payer_name);
        self.payer_note = payer_note.unwrap_or(self.payer_note);
        self.priority = priority.unwrap_or(self.priority);
        self.quantity = quantity.unwrap_or(self.quantity);
        self.replacement_txid =
            replacement_txid.unwrap_or(self.replacement_txid);

        self
    }
}

impl PaymentMetadataUpdate {
    #[allow(dead_code)] // TODO(max): Remove
    pub fn is_empty(&self) -> bool {
        // We intentionally destructure here to ensure we get a compilation
        // error whenever we add another field
        let Self {
            new_related_ids,
            address,
            invoice,
            offer,
            note,
            payer_name,
            payer_note,
            priority,
            quantity,
            replacement_txid,
        } = self;

        new_related_ids.is_empty()
            && address.is_none()
            && invoice.is_none()
            && offer.is_none()
            && note.is_none()
            && payer_name.is_none()
            && payer_note.is_none()
            && priority.is_none()
            && quantity.is_none()
            && replacement_txid.is_none()
    }
}

// --- impl Payment --- //

impl PaymentV2 {
    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::OnchainSend(os) => LxPaymentId::OnchainSend(os.cid),
            Self::OnchainReceive(or) => LxPaymentId::OnchainRecv(or.txid),
            Self::InboundInvoice(iip) => LxPaymentId::Lightning(iip.hash),
            Self::InboundOfferReusable(iorp) =>
                LxPaymentId::OfferRecvReusable(iorp.claim_id),
            Self::InboundSpontaneous(isp) => LxPaymentId::Lightning(isp.hash),
            Self::OutboundInvoice(oip) => LxPaymentId::Lightning(oip.hash),
            Self::OutboundOffer(oop) => LxPaymentId::OfferSend(oop.client_id),
            Self::OutboundSpontaneous(osp) => LxPaymentId::Lightning(osp.hash),
        }
    }

    /// Returns the id of the BOLT12 offer associated with this payment, if
    /// there is one.
    pub fn offer_id(&self) -> Option<LxOfferId> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(_) => None,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                offer_id,
                ..
            }) => Some(*offer_id),
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(_) => None,
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                offer_id, ..
            }) => Some(*offer_id),
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// Returns the original txid, if there is one.
    pub fn txid(&self) -> Option<LxTxid> {
        match self {
            PaymentV2::OnchainSend(OnchainSendV2 { txid, .. }) => Some(*txid),
            PaymentV2::OnchainReceive(OnchainReceiveV2 { txid, .. }) =>
                Some(*txid),
            PaymentV2::InboundInvoice(_) => None,
            PaymentV2::InboundOfferReusable(_) => None,
            PaymentV2::InboundSpontaneous(_) => None,
            PaymentV2::OutboundInvoice(_) => None,
            PaymentV2::OutboundOffer(_) => None,
            PaymentV2::OutboundSpontaneous(_) => None,
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

    /// The sub-kind of this payment, which is exposed for efficient queries.
    pub fn class(&self) -> PaymentClass {
        match self {
            Self::OnchainSend(os) => os.class,
            Self::OnchainReceive(or) => or.class,
            Self::InboundInvoice(iip) => iip.class,
            Self::InboundOfferReusable(iorp) => iorp.class,
            Self::InboundSpontaneous(isp) => isp.class,
            Self::OutboundInvoice(oip) => oip.class,
            Self::OutboundOffer(oop) => oop.class,
            Self::OutboundSpontaneous(osp) => osp.class,
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

    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, we return the amount
    ///   we received.
    /// - If this is a pending or failed inbound inbound invoice payment, we
    ///   return the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always returned.
    pub fn amount(&self) -> Option<Amount> {
        match self {
            Self::OnchainSend(OnchainSendV2 { amount, .. }) => Some(*amount),
            Self::OnchainReceive(OnchainReceiveV2 { amount, .. }) =>
                Some(*amount),
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                invoice_amount,
                recvd_amount,
                ..
            }) => recvd_amount.or(*invoice_amount),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                amount,
                ..
            }) => Some(*amount),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                amount,
                ..
            }) => Some(*amount),
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                amount, ..
            }) => Some(*amount),
            Self::OutboundOffer(OutboundOfferPaymentV2 { amount, .. }) =>
                Some(*amount),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                amount,
                ..
            }) => Some(*amount),
        }
    }

    /// The fees paid or expected to be paid for this payment.
    pub fn fee(&self) -> Amount {
        match self {
            Self::OnchainSend(OnchainSendV2 { onchain_fee, .. }) =>
                *onchain_fee,
            // We don't pay anything to receive money onchain
            Self::OnchainReceive(OnchainReceiveV2 { .. }) => Amount::ZERO,
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                skimmed_fee,
                ..
            }) => skimmed_fee.unwrap_or(Amount::ZERO),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                skimmed_fee,
                ..
            }) => skimmed_fee.unwrap_or(Amount::ZERO),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                skimmed_fee,
                ..
            }) => skimmed_fee.unwrap_or(Amount::ZERO),
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                routing_fee,
                ..
            }) => *routing_fee,
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                routing_fee, ..
            }) => *routing_fee,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                routing_fee,
                ..
            }) => *routing_fee,
        }
    }

    // TODO(max): Implement JIT channel fees
    // /// The portion of the skimmed amount that was used to cover the on-chain
    // /// fees incurred by a JIT channel opened to receive this payment.
    // /// None if no channel fees were incurred.
    // pub fn channel_fee(&self) -> Option<Amount> {
    //     match self {
    //         Self::OnchainSend(_) => None,
    //         Self::OnchainReceive(_) => None,
    //         Self::InboundInvoice(InboundInvoicePaymentV2 {
    //             channel_fee,
    //             ..
    //         }) => *channel_fee,
    //         Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
    //             channel_fee,
    //             ..
    //         }) => *channel_fee,
    //         Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
    //             channel_fee,
    //             ..
    //         }) => *channel_fee,
    //         Self::OutboundInvoice(_) => None,
    //         Self::OutboundOffer(_) => None,
    //         Self::OutboundSpontaneous(_) => None,
    //     }
    // }

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    pub fn status(&self) -> PaymentStatus {
        match self {
            Self::OnchainSend(OnchainSendV2 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::OnchainReceive(OnchainReceiveV2 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                status, ..
            }) => PaymentStatus::from(*status),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                status,
                ..
            }) => PaymentStatus::from(*status),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                status,
                ..
            }) => PaymentStatus::from(*status),
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                status, ..
            }) => PaymentStatus::from(*status),
            Self::OutboundOffer(OutboundOfferPaymentV2 { status, .. }) =>
                PaymentStatus::from(*status),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                status,
                ..
            }) => PaymentStatus::from(*status),
        }
    }

    /// Get the payment status as a human-readable `&'static str`
    pub fn status_str(&self) -> &str {
        match self {
            Self::OnchainSend(OnchainSendV2 { status, .. }) => status.as_str(),
            Self::OnchainReceive(OnchainReceiveV2 { status, .. }) =>
                status.as_str(),
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                status, ..
            }) => status.as_str(),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                status,
                ..
            }) => status.as_str(),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                status,
                ..
            }) => status.as_str(),
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                status,
                failure,
                ..
            }) => failure
                .map(|f| f.as_str())
                .unwrap_or_else(|| status.as_str()),
            Self::OutboundOffer(OutboundOfferPaymentV2 { status, .. }) =>
                status.as_str(),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                status,
                ..
            }) => status.as_str(),
        }
    }

    /// Returns the transaction, if there is one.
    /// Always [`Some`] for on-chain sends and receives.
    pub fn tx(&self) -> Option<Arc<bitcoin::Transaction>> {
        match self {
            PaymentV2::OnchainSend(OnchainSendV2 { tx, .. }) =>
                Some(tx.clone()),
            PaymentV2::OnchainReceive(OnchainReceiveV2 { tx, .. }) =>
                Some(tx.clone()),
            PaymentV2::InboundInvoice(_) => None,
            PaymentV2::InboundOfferReusable(_) => None,
            PaymentV2::InboundSpontaneous(_) => None,
            PaymentV2::OutboundInvoice(_) => None,
            PaymentV2::OutboundOffer(_) => None,
            PaymentV2::OutboundSpontaneous(_) => None,
        }
    }

    /// When this payment was created.
    ///
    /// The `created_at` timestamp is set when the payment is persisted for the
    /// first time; it is guaranteed to be `Some` thereafter.
    pub fn created_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(OnchainSendV2 { created_at, .. }) => *created_at,
            Self::OnchainReceive(OnchainReceiveV2 { created_at, .. }) =>
                *created_at,
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                created_at,
                ..
            }) => *created_at,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                created_at,
                ..
            }) => *created_at,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                created_at,
                ..
            }) => *created_at,
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                created_at, ..
            }) => *created_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                created_at,
                ..
            }) => *created_at,
        }
    }

    /// Set the `created_at` timestamp when the payment is first persisted.
    ///
    /// Idempotent; only works once; subsequent calls have no effect.
    pub fn set_created_at_once(&mut self, created_at: TimestampMs) {
        match self {
            Self::OnchainSend(OnchainSendV2 {
                created_at: field, ..
            }) => field.get_or_insert(created_at),
            Self::OnchainReceive(OnchainReceiveV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
                created_at: field,
                ..
            }) => field.get_or_insert(created_at),
        };
    }

    /// When this payment expires.
    ///
    /// For invoices, this is the invoice expiry time. For offers, this is the
    /// offer's absolute expiry time. Returns `None` if the payment type does
    /// not have an expiration or if the expiry timestamp overflows.
    pub fn expires_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(_) => None,
            Self::OnchainReceive(_) => None,
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                expires_at,
                ..
            }) => *expires_at,
            Self::InboundOfferReusable(_) => None,
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                expires_at,
                ..
            }) => *expires_at,
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                expires_at, ..
            }) => *expires_at,
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// When this payment was completed or failed.
    pub fn finalized_at(&self) -> Option<TimestampMs> {
        match self {
            Self::OnchainSend(OnchainSendV2 { finalized_at, .. }) =>
                *finalized_at,
            Self::OnchainReceive(OnchainReceiveV2 { finalized_at, .. }) =>
                *finalized_at,
            Self::InboundInvoice(InboundInvoicePaymentV2 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::InboundOfferReusable(InboundOfferReusablePaymentV2 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::InboundSpontaneous(InboundSpontaneousPaymentV2 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundInvoice(OutboundInvoicePaymentV2 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundOffer(OutboundOfferPaymentV2 {
                finalized_at,
                ..
            }) => *finalized_at,
            Self::OutboundSpontaneous(OutboundSpontaneousPaymentV2 {
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

impl From<InboundOfferReusablePaymentStatus> for PaymentStatus {
    fn from(specific_status: InboundOfferReusablePaymentStatus) -> Self {
        match specific_status {
            InboundOfferReusablePaymentStatus::Claiming => Self::Pending,
            InboundOfferReusablePaymentStatus::Completed => Self::Completed,
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

impl From<OutboundOfferPaymentStatus> for PaymentStatus {
    fn from(specific_status: OutboundOfferPaymentStatus) -> Self {
        match specific_status {
            OutboundOfferPaymentStatus::Pending => Self::Pending,
            OutboundOfferPaymentStatus::Abandoning => Self::Pending,
            OutboundOfferPaymentStatus::Completed => Self::Completed,
            OutboundOfferPaymentStatus::Failed => Self::Failed,
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

impl InboundOfferReusablePaymentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Claiming => "claiming",
            Self::Completed => "completed",
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

impl OutboundOfferPaymentStatus {
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
mod arbitrary_helpers {
    use std::sync::Arc;

    use proptest::{option, prelude::any, strategy::Strategy};

    use super::*;

    pub fn any_option_arc_invoice()
    -> impl Strategy<Value = Option<Arc<LxInvoice>>> {
        option::of(any::<LxInvoice>()).prop_map(|opt| opt.map(Arc::new))
    }

    pub fn any_option_arc_offer() -> impl Strategy<Value = Option<Arc<LxOffer>>>
    {
        option::of(any::<LxOffer>()).prop_map(|opt| opt.map(Arc::new))
    }
}

#[cfg(test)]
mod test {
    use std::{cmp, fs, path::Path};

    use common::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::{
        arbitrary::any, prop_assert_eq, proptest, strategy::Strategy,
        test_runner::Config,
    };

    use super::*;

    #[test]
    fn payment_serde_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<PaymentV2>();
    }

    // TODO(max): Add encryption roundtrips for v2 types

    #[test]
    fn payment_id_equivalence() {
        let cfg = Config::with_cases(100);

        proptest!(cfg, |(payment: PaymentV2)| {
            let id = match &payment {
                PaymentV2::OnchainSend(x) => x.id(),
                PaymentV2::OnchainReceive(x) => x.id(),
                PaymentV2::InboundInvoice(x) => x.id(),
                PaymentV2::InboundOfferReusable(x) => x.id(),
                PaymentV2::InboundSpontaneous(x) => x.id(),
                PaymentV2::OutboundInvoice(x) => x.id(),
                PaymentV2::OutboundOffer(x) => x.id(),
                PaymentV2::OutboundSpontaneous(x) => x.id(),
            };
            prop_assert_eq!(id, payment.id());
        });
    }

    #[test]
    fn v2_subtypes_serde_roundtrips() {
        use roundtrip::json_value_custom;
        let config = Config::with_cases(16);
        json_value_custom(any::<OnchainSendV2>(), config.clone());
        json_value_custom(any::<OnchainReceiveV2>(), config.clone());
        // TODO(max): Add SpliceIn
        // TODO(max): Add SpliceOut
        json_value_custom(any::<InboundInvoicePaymentV2>(), config.clone());
        json_value_custom(
            any::<InboundOfferReusablePaymentV2>(),
            config.clone(),
        );
        json_value_custom(any::<InboundSpontaneousPaymentV2>(), config.clone());
        json_value_custom(any::<OutboundInvoicePaymentV2>(), config.clone());
        json_value_custom(any::<OutboundOfferPaymentV2>(), config.clone());
        json_value_custom(any::<OutboundSpontaneousPaymentV2>(), config);
    }

    /// Dumps a JSON array of `Payment`s using the proptest strategy.
    /// Generates N of each payment sub-type to ensure even coverage.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored take_payments_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn take_payments_v2_snapshot() {
        const COUNT: usize = 5;
        let seed = 20250316; // Base seed for all variants
        let mut rng = FastRng::from_u64(seed);
        let mut payments = Vec::new();

        // Generate COUNT of each payment type for even coverage
        payments.extend(
            arbitrary::gen_value_iter(&mut rng, any::<OnchainSendV2>())
                .take(COUNT)
                .map(PaymentV2::OnchainSend),
        );
        payments.extend(
            arbitrary::gen_value_iter(&mut rng, any::<OnchainReceiveV2>())
                .take(COUNT)
                .map(PaymentV2::OnchainReceive),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundInvoicePaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::InboundInvoice),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundOfferReusablePaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::InboundOfferReusable),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<InboundSpontaneousPaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::InboundSpontaneous),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundInvoicePaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::OutboundInvoice),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundOfferPaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::OutboundOffer),
        );
        payments.extend(
            arbitrary::gen_value_iter(
                &mut rng,
                any::<OutboundSpontaneousPaymentV2>(),
            )
            .take(COUNT)
            .map(PaymentV2::OutboundSpontaneous),
        );

        println!("---");
        println!("{}", serde_json::to_string_pretty(&payments).unwrap());
        println!("---");
    }

    /// Generate serialized `BasicPaymentV2` sample json data:
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln -- gen_basic_payment_sample_data --ignored --nocapture
    /// ```
    /// NOTE: this lives here b/c `common` can't depend on `lexe-ln`.
    // TODO(max): This test won't be useful until all logic is migrated to
    // PaymentV2, and we've finalized the PaymentV2 + PaymentMetadata
    // serialization format.
    #[test]
    #[ignore]
    fn take_basic_payment_v2_snapshot() {
        let mut rng = FastRng::from_u64(202503031636);
        const N: usize = 3;

        // generate `N` samples for each variant to ensure we get full coverage
        let strategies = vec![
            (
                "OnchainSend",
                any::<OnchainSendV2>()
                    .prop_map(PaymentV2::OnchainSend)
                    .boxed(),
            ),
            (
                "OnchainReceive",
                any::<OnchainReceiveV2>()
                    .prop_map(PaymentV2::OnchainReceive)
                    .boxed(),
            ),
            (
                "InboundInvoice",
                any::<InboundInvoicePaymentV2>()
                    .prop_map(PaymentV2::InboundInvoice)
                    .boxed(),
            ),
            (
                "InboundOfferReusable",
                any::<InboundOfferReusablePaymentV2>()
                    .prop_map(PaymentV2::InboundOfferReusable)
                    .boxed(),
            ),
            (
                "InboundSpontaneous",
                any::<InboundSpontaneousPaymentV2>()
                    .prop_map(PaymentV2::InboundSpontaneous)
                    .boxed(),
            ),
            (
                "OutboundInvoice",
                any::<OutboundInvoicePaymentV2>()
                    .prop_map(PaymentV2::OutboundInvoice)
                    .boxed(),
            ),
            (
                "OutboundOfferPayment",
                any::<OutboundOfferPaymentV2>()
                    .prop_map(PaymentV2::OutboundOffer)
                    .boxed(),
            ),
            (
                "OutboundSpontaneous",
                any::<OutboundSpontaneousPaymentV2>()
                    .prop_map(PaymentV2::OutboundSpontaneous)
                    .boxed(),
            ),
        ];

        for (name, strat) in strategies {
            println!("--- {name}");
            let any_metadata = any::<PaymentMetadata>();
            let any_created_at = any::<TimestampMs>();
            let any_updated_at = any::<TimestampMs>();
            let combined_strat =
                (strat, any_metadata, any_created_at, any_updated_at);

            for (value, metadata, created_at, updated_at_raw) in
                arbitrary::gen_value_iter(&mut rng, combined_strat).take(N)
            {
                // Ensure updated_at >= created_at
                let updated_at = cmp::max(created_at, updated_at_raw);

                // serialize app BasicPaymentV2
                let pwm = PaymentWithMetadata {
                    payment: value,
                    metadata,
                };
                let basic = pwm.into_basic_payment(created_at, updated_at);
                let json = serde_json::to_string(&basic).unwrap();
                println!("{json}");
            }
        }
    }

    // TODO(max): Enable snapshot test after v2 serialization format finalized.
    #[ignore]
    #[test]
    fn payment_v2_snapshot_test() {
        let snapshot_path = Path::new("data/payment-snapshot.v2.json");
        let snapshot = fs::read_to_string(snapshot_path)
            .expect("Failed to read payment snapshot");
        serde_json::from_str::<Vec<PaymentV2>>(&snapshot)
            .expect("Failed to deserialize payment snapshot");
    }
}
