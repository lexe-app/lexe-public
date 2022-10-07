use std::fmt::{self, Display};

use lightning::ln::channelmanager::PaymentSendFailure;
use lightning::ln::msgs::LightningError;
use lightning::ln::{PaymentPreimage, PaymentSecret};
use lightning_invoice::payment::PaymentError;

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: MillisatAmount,
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

// TODO(max): This struct doesn't seem important - perhaps it can be removed?
pub struct MillisatAmount(pub Option<u64>);

impl Display for MillisatAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(amt) => write!(f, "{amt}"),
            None => write!(f, "unknown"),
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
    #[error("Failed to find route: {}", .0.err)]
    Routing(LightningError),
    #[error("Payment send failure: {0:?}")]
    Sending(PaymentSendFailure),
}

impl From<PaymentError> for LxPaymentError {
    fn from(ldk_err: PaymentError) -> Self {
        match ldk_err {
            PaymentError::Invoice(inner) => Self::Invoice(inner),
            PaymentError::Routing(inner) => Self::Routing(inner),
            PaymentError::Sending(inner) => Self::Sending(inner),
        }
    }
}
