//! Lexe payments types and logic.
//!
//! This module is the 'complex' counterpart to the simpler types exposed in
//! [`common::ln::payments`].

use std::fmt::{self, Display};

use common::ln::invoice::LxInvoice;
use common::ln::payments::{
    BasicPayment, LxPaymentId, PaymentDirection, PaymentKind, PaymentStatus,
};
use common::time::TimestampMs;
use lightning::ln::channelmanager::PaymentSendFailure;
use lightning::ln::{PaymentPreimage, PaymentSecret};
use lightning_invoice::payment::PaymentError;
use serde::{Deserialize, Serialize};

use crate::payments::inbound::{
    InboundInvoicePayment, InboundInvoicePaymentStatus,
    InboundSpontaneousPayment, InboundSpontaneousPaymentStatus,
};
use crate::payments::onchain::{OnchainDeposit, OnchainWithdrawal};
use crate::payments::outbound::{
    OutboundInvoicePayment, OutboundInvoicePaymentStatus,
    OutboundSpontaneousPayment, OutboundSpontaneousPaymentStatus,
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
/// NOTE: Everything in this in this enum impls [`Serialize`] and
/// [`Deserialize`], so be mindful of backwards compatibility.
#[derive(Clone, Serialize, Deserialize)]
pub enum Payment {
    OnchainDeposit(OnchainDeposit),
    OnchainWithdrawal(OnchainWithdrawal),
    // TODO(max): Implement SpliceIn
    // TODO(max): Implement SpliceOut
    InboundInvoice(InboundInvoicePayment),
    InboundSpontaneous(InboundSpontaneousPayment),
    OutboundInvoice(OutboundInvoicePayment),
    OutboundSpontaneous(OutboundSpontaneousPayment),
}

// --- Specific payment type -> top-level Payment types --- //

impl From<OnchainDeposit> for Payment {
    fn from(p: OnchainDeposit) -> Self {
        Self::OnchainDeposit(p)
    }
}
impl From<OnchainWithdrawal> for Payment {
    fn from(p: OnchainWithdrawal) -> Self {
        Self::OnchainWithdrawal(p)
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
            id: p.id(),
            kind: p.kind(),
            direction: p.direction(),
            invoice: p.invoice(),
            amt_msat: p.amt_msat(),
            fees_msat: p.fees_msat(),
            status: p.status(),
            status_str: p.status_str().to_owned(),
            created_at: p.created_at(),
            finalized_at: p.finalized_at(),
        }
    }
}

// --- impl Payment --- //

impl Payment {
    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::OnchainDeposit(od) => LxPaymentId::Onchain(od.txid),
            Self::OnchainWithdrawal(ow) => LxPaymentId::Onchain(ow.txid),
            Self::InboundInvoice(iip) => LxPaymentId::Lightning(iip.hash),
            Self::InboundSpontaneous(isp) => LxPaymentId::Lightning(isp.hash),
            Self::OutboundInvoice(oip) => LxPaymentId::Lightning(oip.hash),
            Self::OutboundSpontaneous(osp) => LxPaymentId::Lightning(osp.hash),
        }
    }

    /// Whether this is an onchain payment, LN invoice payment, etc.
    pub fn kind(&self) -> PaymentKind {
        match self {
            Self::OnchainDeposit(_) => PaymentKind::Onchain,
            Self::OnchainWithdrawal(_) => PaymentKind::Onchain,
            Self::InboundInvoice(_) => PaymentKind::Invoice,
            Self::InboundSpontaneous(_) => PaymentKind::Spontaneous,
            Self::OutboundInvoice(_) => PaymentKind::Invoice,
            Self::OutboundSpontaneous(_) => PaymentKind::Spontaneous,
        }
    }

    /// Whether this payment is inbound or outbound. Useful for filtering.
    pub fn direction(&self) -> PaymentDirection {
        match self {
            Self::OnchainDeposit(_) => PaymentDirection::Inbound,
            Self::OnchainWithdrawal(_) => PaymentDirection::Outbound,
            Self::InboundInvoice(_) => PaymentDirection::Inbound,
            Self::InboundSpontaneous(_) => PaymentDirection::Inbound,
            Self::OutboundInvoice(_) => PaymentDirection::Outbound,
            Self::OutboundSpontaneous(_) => PaymentDirection::Outbound,
        }
    }

    /// Returns the invoice corresponding to this payment, if there is one.
    pub fn invoice(&self) -> Option<LxInvoice> {
        match self {
            Self::OnchainDeposit(_) => None,
            Self::OnchainWithdrawal(_) => None,
            Self::InboundInvoice(InboundInvoicePayment { invoice, .. }) => {
                Some(*invoice.clone())
            }
            Self::InboundSpontaneous(_) => None,
            Self::OutboundInvoice(OutboundInvoicePayment {
                invoice, ..
            }) => Some(*invoice.clone()),
            Self::OutboundSpontaneous(_) => None,
        }
    }

    /// The amount of this payment in millisatoshis.
    ///
    /// - If this is a completed inbound invoice payment, we return the amount
    ///   we received.
    /// - If this is a pending or failed inbound inbound invoice payment, we
    ///   return the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always returned.
    // TODO(max): Use LDK-provided Amount newtype when available
    pub fn amt_msat(&self) -> Option<u64> {
        match self {
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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
    pub fn fees_msat(&self) -> u64 {
        match self {
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    pub fn status(&self) -> PaymentStatus {
        match self {
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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
    pub fn status_str(&self) -> &str {
        match self {
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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
    pub fn created_at(&self) -> TimestampMs {
        match self {
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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
            Self::OnchainDeposit(_) => todo!(),
            Self::OnchainWithdrawal(_) => todo!(),
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
        // All finalized payments must have a finalized_at() timestamp.
        if matches!(
            self.status(),
            PaymentStatus::Completed | PaymentStatus::Failed
        ) {
            assert!(self.finalized_at().is_some());
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
    pub fn as_str(&self) -> &str {
        match self {
            Self::InvoiceGenerated => "invoice generated",
            Self::Claiming => "claiming",
            Self::Completed => "completed",
            Self::TimedOut => "timed out",
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
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed out",
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
// --- Types inherited from ldk-sample --- //
// TODO(max): Gradually remove / replace these with our own

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: Option<u64>,
}

#[allow(dead_code)]
pub enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

impl Display for HTLCStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A newtype for [`PaymentError`] that impls [`Display`] and [`Error`].
///
/// [`Error`]: std::error::Error
#[derive(Debug, thiserror::Error)]
pub enum LxPaymentError {
    #[error("Invalid invoice: {0}")]
    Invoice(&'static str),
    #[error("Payment send failure: {0:?}")]
    Sending(Box<PaymentSendFailure>),
}

impl From<PaymentError> for LxPaymentError {
    fn from(ldk_err: PaymentError) -> Self {
        match ldk_err {
            PaymentError::Invoice(inner) => Self::Invoice(inner),
            PaymentError::Sending(inner) => Self::Sending(Box::new(inner)),
        }
    }
}
