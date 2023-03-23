//! Payment types.
//!
//! The full hierarchy of payments types is as follows:
//!
//! [`Payment`]
//! |
//! |___[`OnchainDeposit`]
//! |
//! |___[`OnchainWithdrawal`]
//! |
//! |___`SpliceIn` TODO(max): Implement
//! |
//! |___`SpliceOut` TODO(max): Implement
//! |
//! |___[`InboundInvoicePayment`]
//! |
//! |___[`InboundSpontaneousPayment`]
//! |
//! |___[`OutboundInvoicePayment`]
//! |
//! |___[`OutboundSpontaneousPayment`]
//!
//! NOTE: Everything in this hierarchy impls [`Serialize`] and [`Deserialize`],
//! so be mindful of backwards compatibility.
//!
//! [`Payment`]: payments::Payment
//! [`OnchainDeposit`]: payments::onchain::OnchainDeposit
//! [`OnchainWithdrawal`]: payments::onchain::OnchainWithdrawal
//! [`InboundInvoicePayment`]: payments::inbound::InboundInvoicePayment
//! [`InboundSpontaneousPayment`]: payments::inbound::InboundSpontaneousPayment
//! [`OutboundInvoicePayment`]: payments::outbound::OutboundInvoicePayment
//! [`OutboundSpontaneousPayment`]: payments::outbound::OutboundSpontaneousPayment
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize

use std::fmt::{self, Display};

use lightning::ln::channelmanager::PaymentSendFailure;
use lightning::ln::{PaymentPreimage, PaymentSecret};
use lightning_invoice::payment::PaymentError;
use serde::{Deserialize, Serialize};

use crate::payments::inbound::{
    InboundInvoicePayment, InboundSpontaneousPayment,
};
use crate::payments::onchain::{OnchainDeposit, OnchainWithdrawal};
use crate::payments::outbound::{
    OutboundInvoicePayment, OutboundSpontaneousPayment,
};

/// Contains the boring / repetitive / tedious code for `Payment`'s getters.
pub mod getters;
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
// See `getters` for the main `Payment` impl.
#[derive(Clone, Serialize, Deserialize)]
pub enum Payment {
    OnchainDeposit(OnchainDeposit),
    OnchainWithdrawal(OnchainWithdrawal),
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
