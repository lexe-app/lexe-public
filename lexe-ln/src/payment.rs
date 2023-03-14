use std::fmt::{self, Display};

use lightning::ln::channelmanager::PaymentSendFailure;
use lightning::ln::{PaymentPreimage, PaymentSecret};
use lightning_invoice::payment::PaymentError;

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
