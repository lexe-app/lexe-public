use common::time::TimestampMillis;
use serde::{Deserialize, Serialize};

use crate::payments::offchain::inbound::{
    InboundInvoicePayment, InboundInvoicePaymentStatus,
    InboundSpontaneousPayment, InboundSpontaneousPaymentStatus,
};
use crate::payments::offchain::outbound::{
    OutboundInvoicePayment, OutboundInvoicePaymentStatus,
    OutboundSpontaneousPayment, OutboundSpontaneousPaymentStatus,
};
use crate::payments::{
    LxPaymentHash, PaymentDirection, PaymentStatus, PaymentTrait,
};

/// Detailed types and state machines for inbound Lightning payments.
pub mod inbound;
/// Detailed types and state machines for outbound Lightning payments.
pub mod outbound;

/// Abstracts over all Lightning payments and provides convenience methods for
/// matching on the contained struct.
#[derive(Clone, Serialize, Deserialize)]
pub enum LightningPayment {
    InboundInvoice(InboundInvoicePayment),
    InboundSpontaneous(InboundSpontaneousPayment),
    OutboundInvoice(OutboundInvoicePayment),
    OutboundSpontaneous(OutboundSpontaneousPayment),
}

impl LightningPayment {
    /// Get the contained [`LxPaymentHash`].
    pub fn hash(&self) -> &LxPaymentHash {
        match self {
            Self::InboundInvoice(InboundInvoicePayment { hash, .. }) => hash,
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                hash,
                ..
            }) => hash,
            Self::OutboundInvoice(OutboundInvoicePayment { hash, .. }) => hash,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                hash,
                ..
            }) => hash,
        }
    }
}

impl PaymentTrait for LightningPayment {
    /// Whether this payment is inbound or outbound. Useful for filtering.
    fn direction(&self) -> PaymentDirection {
        match self {
            Self::InboundInvoice(..) => PaymentDirection::Inbound,
            Self::InboundSpontaneous(..) => PaymentDirection::Inbound,
            Self::OutboundInvoice(..) => PaymentDirection::Outbound,
            Self::OutboundSpontaneous(..) => PaymentDirection::Outbound,
        }
    }

    /// The amount of this payment in millisatoshis.
    ///
    /// - If this is a completed inbound invoice payment, we return the amount
    ///   we received.
    /// - If this is a pending or failed inbound inbound invoice payment, we
    ///   return the amount encoded in our invoice (if there was one).
    /// - For all other payment types, an amount is always returned.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn amt_msat(&self) -> Option<u64> {
        match self {
            Self::InboundInvoice(InboundInvoicePayment {
                invoice_amt_msat,
                recvd_amount_msat,
                ..
            }) => recvd_amount_msat.or(*invoice_amt_msat),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                amt_msat,
                ..
            }) => Some(*amt_msat),
            Self::OutboundInvoice(OutboundInvoicePayment {
                amt_msat, ..
            }) => Some(*amt_msat),
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                amt_msat,
                ..
            }) => Some(*amt_msat),
        }
    }

    /// The fees paid or expected to be paid for this payment.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn fees_msat(&self) -> u64 {
        match self {
            Self::InboundInvoice(InboundInvoicePayment {
                onchain_fees_msat,
                ..
            }) => onchain_fees_msat.unwrap_or(0),
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                onchain_fees_msat,
                ..
            }) => onchain_fees_msat.unwrap_or(0),
            Self::OutboundInvoice(OutboundInvoicePayment {
                fees_msat, ..
            }) => *fees_msat,
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                fees_msat,
                ..
            }) => *fees_msat,
        }
    }

    /// Get a general [`PaymentStatus`] for this payment.
    /// Useful for filtering [`LightningPayment`] by general status.
    fn status(&self) -> PaymentStatus {
        match self {
            Self::InboundInvoice(InboundInvoicePayment { status, .. }) => {
                PaymentStatus::from(*status)
            }
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
    fn status_str(&self) -> &str {
        match self {
            Self::InboundInvoice(InboundInvoicePayment { status, .. }) => {
                status.as_str()
            }
            Self::InboundSpontaneous(InboundSpontaneousPayment {
                status,
                ..
            }) => status.as_str(),
            Self::OutboundInvoice(OutboundInvoicePayment {
                status, ..
            }) => status.as_str(),
            Self::OutboundSpontaneous(OutboundSpontaneousPayment {
                status,
                ..
            }) => status.as_str(),
        }
    }

    /// When this payment was created.
    ///
    /// - For inbound invoice payments, this is when we created the invoice.
    /// - For inbound spontaneous payments, this is when we first learned of the
    ///   inbound payment.
    /// - For outbound payments, this is when we first initiated the payment.
    fn created_at(&self) -> TimestampMillis {
        match self {
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

    /// When this payment was completed, failed, or timed out.
    ///
    /// - Inbound invoice payments can time out.
    /// - Inbound spontaneous payments should always complete.
    /// - Outbound invoice payments can fail, or the recipient's invoice may
    ///   expire before we manage to complete the payment.
    /// - Outbound spontaneous payments can fail.
    fn finalized_at(&self) -> Option<TimestampMillis> {
        match self {
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
}

// --- Payment-specific status -> General PaymentStatus  --- //

impl From<InboundInvoicePaymentStatus> for PaymentStatus {
    fn from(specific_status: InboundInvoicePaymentStatus) -> Self {
        match specific_status {
            InboundInvoicePaymentStatus::InvoiceGenerated => Self::Pending,
            InboundInvoicePaymentStatus::Claiming => Self::Pending,
            InboundInvoicePaymentStatus::Completed => Self::Completed,
            InboundInvoicePaymentStatus::TimedOut => Self::Failed,
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
            OutboundInvoicePaymentStatus::Completed => Self::Completed,
            OutboundInvoicePaymentStatus::Failed => Self::Failed,
            OutboundInvoicePaymentStatus::TimedOut => Self::Failed,
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

impl InboundInvoicePaymentStatus {
    fn as_str(&self) -> &str {
        match self {
            Self::InvoiceGenerated => "invoice generated",
            Self::Claiming => "claiming",
            Self::Completed => "completed",
            Self::TimedOut => "timed out",
        }
    }
}

impl InboundSpontaneousPaymentStatus {
    fn as_str(&self) -> &str {
        match self {
            Self::Claiming => "claiming",
            Self::Completed => "completed",
        }
    }
}

impl OutboundInvoicePaymentStatus {
    fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed out",
        }
    }
}

impl OutboundSpontaneousPaymentStatus {
    fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}
