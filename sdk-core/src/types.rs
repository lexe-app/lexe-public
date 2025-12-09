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

use common::{
    ln::{amount::Amount, hashes::LxTxid},
    time::TimestampMs,
};
use lexe_api_core::types::payments::{
    PaymentCreatedIndex, PaymentDirection, PaymentKind, PaymentStatus,
};
use serde::{Deserialize, Serialize};

/// Information about a payment.
#[derive(Serialize, Deserialize)]
pub struct SdkPayment {
    /// Identifier for this payment.
    pub index: PaymentCreatedIndex,

    /// Application-level payment kind.
    pub kind: PaymentKind,

    /// The payment direction: ["inbound", "outbound"].
    pub direction: PaymentDirection,

    // NOTE: Excluding `invoice` for now as externally-generated invoices are
    // unknown to the node, but perhaps the sidecar could cache / persist it?
    // Invoices will probably also be stored separately from the main payment.
    /*
    /// (Invoice payments only) The BOLT11 invoice used in this payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<Box<LxInvoice>>,
    */

    // TODO(max): Unclear if we'll always have access to the offer_id or offer
    // when we receive an offer payment. Leaving this out until we're more sure.
    /*
    /// (Offer payments only) The id of the BOLT12 offer used in this payment.
    pub offer_id: Option<LxOfferId>,

    /// (Outbound offer payments only) The BOLT12 offer used in this payment.
    /// Until we store offers out-of-line, this is not yet available for
    /// inbound offer payments.
    pub offer: Option<Box<LxOffer>>,
    */
    /// (Onchain payments only) The hex-encoded Bitcoin txid.
    pub txid: Option<LxTxid>,

    /// (Onchain payments only) The hex-encoded txid of the transaction that
    /// spent the outputs spent by this on-chain payment, if one exists.
    pub replacement: Option<LxTxid>,

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

    /// An optional personal note which a user can attach to any payment.
    /// A note can always be added or modified when a payment already exists,
    /// but this may not always be possible at creation time.
    pub note: Option<String>,

    /// If this payment is finalized, meaning it is "completed" or "failed",
    /// this is the time it was finalized, in milliseconds since the UNIX
    /// epoch.
    pub finalized_at: Option<TimestampMs>,
}
