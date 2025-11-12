//! Lexe payments types and logic.
//!
//! This module is the 'complex' counterpart to the simpler types exposed in
//! [`lexe_api::types::payments`].

use anyhow::Context;
use common::{aes::AesMasterKey, rng::Crng, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{DbPaymentV2, LxPaymentId, PaymentStatus},
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::payments::{
    inbound::{
        InboundInvoicePaymentStatus, InboundOfferReusablePaymentStatus,
        InboundSpontaneousPaymentStatus,
    },
    onchain::{OnchainReceiveStatus, OnchainSendStatus},
    outbound::{
        OutboundInvoicePaymentStatus, OutboundOfferPaymentStatus,
        OutboundSpontaneousPaymentStatus,
    },
    v1::{
        PaymentV1,
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
/// `PaymentsManager`.
pub mod manager;
/// On-chain payment types and state machines.
pub mod onchain;
/// Outbound Lightning payments.
pub mod outbound;
/// `PaymentV1` and sub-types.
pub mod v1;

// --- Top-level payment types --- //

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentWithMetadata {
    pub payment: PaymentV2,
    pub metadata: Option<PaymentMetadata>,

    // TODO(max): We temporarily have to store the `created_at` field here so
    // that we can convert `PaymentV1` -> `PaymentWithMetadata` and back
    // without data loss, since `PaymentV2` drops the `created_at` field.
    pub created_at: TimestampMs,
}

/// Optional payment metadata associated with a [`PaymentV2`].
#[derive(Clone, Debug, Eq, PartialEq)]
// TODO(max): This should derive Serialize, Deserialize. We hold off for now as
// we don't want to accidentally serialize using this type while we're still
// migrating all logic.
pub struct PaymentMetadata {
    pub id: LxPaymentId,

    /// The BOLT11 invoice corresponding to this payment, if any.
    pub invoice: Option<Box<LxInvoice>>,

    /// The BOLT12 offer associated with this payment, if any.
    pub offer: Option<Box<LxOffer>>,

    /// Private the payment note.
    pub note: Option<String>,
    //
    // TODO(max): Add remaining fields once we implement the migration
}

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
pub enum PaymentV2 {
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

// --- Encryption --- //

/// Serializes a given payment to JSON and encrypts the payment under the given
/// [`AesMasterKey`], returning the [`DbPaymentV2`] which can be persisted.
pub fn encrypt(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    // TODO(max): This should take only `&PaymentV2` once logic is migrated.
    // There will also be a separate function `encrypt_metadata` which takes
    // `&PaymentMetadata`.
    pwm: &PaymentWithMetadata,
    updated_at: TimestampMs,
) -> DbPaymentV2 {
    // Serialize the payment as JSON bytes.
    let aad = &[];
    let data_size_hint = None;
    // TODO(max): Update serialization to v2 once all logic is migrated.
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|mut_vec_u8| {
        let payment_v1 = PaymentV1::from(pwm.clone());
        serde_json::to_writer(mut_vec_u8, &payment_v1)
            .expect("Payment serialization always succeeds")
    };

    // Encrypt.
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPaymentV2 {
        id: pwm.payment.id().to_string(),
        status: pwm.payment.status().to_string(),
        data,
        // TODO(max): created_at should come from the persister
        // created_at: created_at.to_i64(),
        created_at: pwm.created_at.to_i64(),
        updated_at: updated_at.to_i64(),
    }
}

/// Given a [`DbPaymentV2::data`] (ciphertext), attempts to decrypt using the
/// given [`AesMasterKey`], returning the deserialized [`PaymentV1`].
// TODO(max): This should return only `PaymentV2` once logic is migrated.
// There will also be a separate function `decrypt_metadata` which returns
// `PaymentMetadata`.
pub fn decrypt(
    vfs_master_key: &AesMasterKey,
    data: Vec<u8>,
) -> anyhow::Result<PaymentWithMetadata> {
    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, data)
        .context("Could not decrypt Payment")?;

    // TODO(max): Update deserialization to v2 once all logic is migrated.
    serde_json::from_slice::<PaymentV1>(plaintext_bytes.as_slice())
        .map(PaymentWithMetadata::from)
        .context("Could not deserialize Payment")
}

// --- Payment subtype -> top-level Payment type --- //

impl From<OnchainSendV1> for PaymentV2 {
    fn from(p: OnchainSendV1) -> Self {
        Self::OnchainSend(p)
    }
}
impl From<OnchainReceiveV1> for PaymentV2 {
    fn from(p: OnchainReceiveV1) -> Self {
        Self::OnchainReceive(p)
    }
}
impl From<InboundInvoicePaymentV1> for PaymentV2 {
    fn from(p: InboundInvoicePaymentV1) -> Self {
        Self::InboundInvoice(p)
    }
}
impl From<InboundOfferReusablePaymentV1> for PaymentV2 {
    fn from(p: InboundOfferReusablePaymentV1) -> Self {
        Self::InboundOfferReusable(p)
    }
}
impl From<InboundSpontaneousPaymentV1> for PaymentV2 {
    fn from(p: InboundSpontaneousPaymentV1) -> Self {
        Self::InboundSpontaneous(p)
    }
}
impl From<OutboundInvoicePaymentV1> for PaymentV2 {
    fn from(p: OutboundInvoicePaymentV1) -> Self {
        Self::OutboundInvoice(p)
    }
}
impl From<OutboundOfferPaymentV1> for PaymentV2 {
    fn from(p: OutboundOfferPaymentV1) -> Self {
        Self::OutboundOffer(p)
    }
}
impl From<OutboundSpontaneousPaymentV1> for PaymentV2 {
    fn from(p: OutboundSpontaneousPaymentV1) -> Self {
        Self::OutboundSpontaneous(p)
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
            Self::OutboundOffer(oop) => LxPaymentId::OfferSend(oop.cid),
            Self::OutboundSpontaneous(osp) => LxPaymentId::Lightning(osp.hash),
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
mod test {
    use common::{aes::AesMasterKey, rng::FastRng};
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;
    use crate::payments;

    #[test]
    fn payment_encryption_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            vfs_master_key in any::<AesMasterKey>(),
            p1 in any::<PaymentV2>(),
            updated_at in any::<TimestampMs>(),
        )| {
            let metadata = None;
            // TODO(max): Remove PaymentWithMetadata later. Dummy value for now.
            let pwm = PaymentWithMetadata {
                payment: p1.clone(),
                metadata,
                // TODO(max): Remove this field later. Dummy value for now.
                created_at: TimestampMs::MIN,
            };

            let encrypted = payments::encrypt(
                &mut rng, &vfs_master_key, &pwm, updated_at
            );
            let p2 = payments::decrypt(&vfs_master_key, encrypted.data)
                .map(|pwm| pwm.payment)
                .unwrap();
            prop_assert_eq!(p1, p2);
        })
    }
}
