//! This module contains common data structures and types used by API request
//! and response types.
//!
//! NOTE: The guidelines in [`sdk_core::models`] should also be followed here:
//!
//! - Simple: minimal nesting, fewer fields
//! - User-facing docs
//! - Document serialization and units
//! - Serialize `null`
//!
//! [`sdk_core::models`]: crate::models

use std::sync::Arc;

use bitcoin::address::NetworkUnchecked;
use common::{
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    time::TimestampMs,
};
use lexe_api_core::types::{
    invoice::LxInvoice,
    payments::{
        BasicPaymentV2, LxPaymentId, PaymentCreatedIndex, PaymentDirection,
        PaymentKind, PaymentRail, PaymentStatus,
    },
};
use serde::{Deserialize, Serialize};

/// Information about a payment.
#[derive(Serialize, Deserialize)]
pub struct SdkPayment {
    /// Unique identifier for this payment, ordered by created_at.
    ///
    /// This implements [`Ord`] and is generally the thing you want to key your
    /// payments by, e.g. `BTreeMap<PaymentCreatedIndex, SdkPayment>`.
    pub index: PaymentCreatedIndex,

    /// Unordered payment identifier.
    /// You should prefer to use [`index`](Self::index) instead of this.
    pub id: LxPaymentId,

    /// The technical 'rail' used to fulfill a payment:
    /// 'onchain', 'invoice', 'offer', 'spontaneous', 'waived_fee', etc.
    pub rail: PaymentRail,

    /// Application-level payment kind.
    pub kind: PaymentKind,

    /// The payment direction: `"inbound"`, `"outbound"`, or `"info"`.
    pub direction: PaymentDirection,

    /* TODO(max): Expose offer_id once we have out-of-line Offer storage.
    /// (Offer payments only) The id of the BOLT12 offer used in this payment.
    pub offer_id: Option<LxOfferId>,
    */
    /// (Onchain payments only) The hex-encoded Bitcoin txid.
    pub txid: Option<LxTxid>,

    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, this is the amount we
    ///   received.
    /// - If this is a pending or failed inbound inbound invoice payment, this
    ///   is the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always included.
    pub amount: Option<Amount>,

    /// The fees for this payment.
    ///
    /// - For outbound Lightning payments, these are the routing fees. If the
    ///   payment is not completed, this value is an estimation only. This
    ///   value reflects the actual fees paid if and only if the payment
    ///   completes.
    /// - For inbound Lightning payments, the routing fees are not paid by us
    ///   (the recipient), but if a JIT channel open was required to facilitate
    ///   this payment, then the on-chain fee is reflected here.
    pub fees: Amount,

    /// The status of this payment: ["pending", "completed", "failed"].
    pub status: PaymentStatus,

    /// The payment status as a human-readable message. These strings are
    /// customized per payment type, e.g. "invoice generated", "timed out"
    pub status_msg: String,

    /// (Onchain send only) The address that we're sending to.
    pub address: Option<Arc<bitcoin::Address<NetworkUnchecked>>>,

    /// (Invoice payments only) The BOLT11 invoice used in this payment.
    pub invoice: Option<Arc<LxInvoice>>,

    /* TODO(max): Expose offer once we have out-of-line Offer storage.
    /// (Outbound offer payments only) The BOLT12 offer used in this payment.
    /// Until we store offers out-of-line, this is not yet available for
    /// inbound offer payments.
    pub offer: Option<Arc<LxOffer>>,
    */
    /// The on-chain transaction, if there is one.
    /// Always [`Some`] for on-chain sends and receives.
    pub tx: Option<Arc<bitcoin::Transaction>>,

    /// An optional personal note which a user can attach to any payment.
    /// A note can always be added or modified when a payment already exists,
    /// but this may not always be possible at creation time.
    pub note: Option<String>,

    /// (Offer payments only) The payer's self-reported human-readable name.
    pub payer_name: Option<String>,

    /// (Offer payments only) A payer-provided note for this payment.
    /// LDK truncates this to 512 bytes.
    pub payer_note: Option<String>,

    /// (Onchain send only) The confirmation priority used for this payment.
    pub priority: Option<ConfirmationPriority>,

    /* TODO(max): Expose replacement_txid once someone cares about it.
    /// (Onchain payments only) The hex-encoded txid of the transaction that
    /// replaced this on-chain payment, if one exists.
    pub replacement_txid: Option<LxTxid>,
    */
    /// The invoice or offer expiry time.
    /// `None` otherwise, or if the timestamp overflows.
    pub expires_at: Option<TimestampMs>,

    /// If this payment is finalized, meaning it is "completed" or "failed",
    /// this is the time it was finalized, in milliseconds since the UNIX
    /// epoch.
    pub finalized_at: Option<TimestampMs>,

    /// When this payment was created.
    pub created_at: TimestampMs,

    /// When this payment was last updated.
    pub updated_at: TimestampMs,
}

impl From<BasicPaymentV2> for SdkPayment {
    fn from(p: BasicPaymentV2) -> Self {
        let BasicPaymentV2 {
            id,
            related_ids: _,
            kind,
            direction,
            offer_id: _,
            txid,
            amount,
            fee,
            status,
            status_str,
            address,
            invoice,
            offer: _,
            tx,
            note,
            payer_name,
            payer_note,
            priority,
            quantity: _,
            replacement_txid: _,
            expires_at,
            finalized_at,
            created_at,
            updated_at,
        } = p;

        let index = PaymentCreatedIndex { created_at, id };

        Self {
            index,
            id,
            rail: kind.rail(),
            kind,
            direction,
            txid,
            amount,
            fees: fee,
            status,
            status_msg: status_str,
            address,
            invoice,
            tx,
            note,
            payer_name,
            payer_note,
            priority,
            expires_at,
            finalized_at,
            created_at,
            updated_at,
        }
    }
}
