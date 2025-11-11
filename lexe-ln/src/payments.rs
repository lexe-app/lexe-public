//! Lexe payments types and logic.
//!
//! This module is the 'complex' counterpart to the simpler types exposed in
//! [`lexe_api::types::payments`].

use anyhow::Context;
use common::{aes::AesMasterKey, rng::Crng, time::TimestampMs};
use lexe_api::types::payments::{DbPaymentV2, PaymentStatus};

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

// --- The top-level payment type --- //

/// Serializes a given payment to JSON and encrypts the payment under the given
/// [`AesMasterKey`], returning the [`DbPaymentV2`] which can be persisted.
pub fn encrypt(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    payment: &PaymentV1,
    updated_at: TimestampMs,
) -> DbPaymentV2 {
    // Serialize the payment as JSON bytes.
    let aad = &[];
    let data_size_hint = None;
    let write_data_cb: &dyn Fn(&mut Vec<u8>) = &|mut_vec_u8| {
        serde_json::to_writer(mut_vec_u8, payment)
            .expect("Payment serialization always succeeds")
    };

    // Encrypt.
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    DbPaymentV2 {
        id: payment.id().to_string(),
        status: payment.status().to_string(),
        data,
        // TODO(max): Remove `created_at` from core `Payment` type
        created_at: payment.created_at().to_i64(),
        updated_at: updated_at.to_i64(),
    }
}

/// Given a [`DbPaymentV2::data`] (ciphertext), attempts to decrypt using the
/// given [`AesMasterKey`], returning the deserialized [`PaymentV1`].
pub fn decrypt(
    vfs_master_key: &AesMasterKey,
    data: Vec<u8>,
) -> anyhow::Result<PaymentV1> {
    let aad = &[];
    let plaintext_bytes = vfs_master_key
        .decrypt(aad, data)
        .context("Could not decrypt Payment")?;

    serde_json::from_slice::<PaymentV1>(plaintext_bytes.as_slice())
        .context("Could not deserialize Payment")
}

// --- Specific payment type -> top-level Payment types --- //

// --- impl Payment --- //

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
