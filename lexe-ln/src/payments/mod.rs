//! Payment types.
//!
//! The full hierarchy of payments types is as follows:
//!
//! [`Payment`]
//! |
//! |___[`OnchainPayment`]
//! |   |
//! |   |___[`OnchainDeposit`]
//! |   |
//! |   |___[`OnchainWithdrawal`]
//! |   |
//! |   |___`SpliceIn` TODO(max): Implement
//! |   |
//! |   |___`SpliceOut` TODO(max): Implement
//! |
//! |___[`LightningPayment`]
//!     |
//!     |___[`InboundInvoicePayment`]
//!     |
//!     |___[`InboundSpontaneousPayment`]
//!     |
//!     |___[`OutboundInvoicePayment`]
//!     |
//!     |___[`OutboundSpontaneousPayment`]
//!
//! NOTE: Everything in this hierarchy impls [`Serialize`] and [`Deserialize`],
//! so be mindful of backwards compatibility.
//!
//! [`Payment`]: payments::Payment
//! [`OnchainPayment`]: payments::onchain::OnchainPayment
//! [`LightningPayment`]: payments::offchain::LightningPayment
//! [`OnchainDeposit`]: payments::onchain::OnchainDeposit
//! [`OnchainWithdrawal`]: payments::onchain::OnchainWithdrawal
//! [`InboundInvoicePayment`]: payments::offchain::inbound::InboundInvoicePayment
//! [`InboundSpontaneousPayment`]: payments::offchain::inbound::InboundSpontaneousPayment
//! [`OutboundInvoicePayment`]: payments::offchain::outbound::OutboundInvoicePayment
//! [`OutboundSpontaneousPayment`]: payments::offchain::outbound::OutboundSpontaneousPayment
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize

use std::fmt::{self, Display};

use common::hexstr_or_bytes;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::channelmanager::{PaymentId, PaymentSendFailure};
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{
    PaymentClaimable, PaymentClaimed, PaymentSent,
};
#[cfg(doc)]
use lightning::util::events::PaymentPurpose;
use lightning_invoice::payment::PaymentError;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::command::get_invoice;
use crate::payments::offchain::LightningPayment;
use crate::payments::onchain::OnchainPayment;

/// `PaymentsManager`.
pub mod manager;
/// Lightning payment types and state machines.
pub mod offchain;
/// On-chain payment types and state machines.
pub mod onchain;

// --- Top-level payment types --- //

/// The top level [`Payment`] type which abstracts over all types of payments,
/// including both onchain and off-chain (Lightning) payments.
#[derive(Clone, Serialize, Deserialize)]
pub enum Payment {
    Onchain(OnchainPayment),
    Lightning(LightningPayment),
}

/// Specifies whether a payment is inbound or outbound.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PaymentDirection {
    Inbound,
    Outbound,
}

/// A general payment status that abstracts over all payment types.
///
/// - Useful for filtering all payments by status in a high-level list view.
/// - Not suitable for getting detailed information about a specific payment; in
///   this case, use the payment-specific status enum or `status_str()` instead.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

// --- Lexe newtypes --- //

/// Newtype for [`PaymentHash`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentHash(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentPreimage`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentPreimage(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentSecret`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentSecret(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

// --- impl Payment --- //

impl Payment {
    /// Whether this payment is inbound or outbound. Useful for filtering.
    pub fn direction(&self) -> PaymentDirection {
        match self {
            Self::Onchain(onchain) => onchain.direction(),
            Self::Lightning(lightning) => lightning.direction(),
        }
    }
}

// --- Redact secret information --- //

impl fmt::Debug for LxPaymentPreimage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LxPaymentPreimage(..)")
    }
}

impl fmt::Debug for LxPaymentSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LxPaymentSecret(..)")
    }
}

// --- Newtype from impls --- //

// LDK -> Lexe
impl From<PaymentHash> for LxPaymentHash {
    fn from(hash: PaymentHash) -> Self {
        Self(hash.0)
    }
}
impl From<PaymentPreimage> for LxPaymentPreimage {
    fn from(preimage: PaymentPreimage) -> Self {
        Self(preimage.0)
    }
}
impl From<PaymentSecret> for LxPaymentSecret {
    fn from(secret: PaymentSecret) -> Self {
        Self(secret.0)
    }
}

// Lexe -> LDK
impl From<LxPaymentHash> for PaymentHash {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
    }
}
impl From<LxPaymentPreimage> for PaymentPreimage {
    fn from(preimage: LxPaymentPreimage) -> Self {
        Self(preimage.0)
    }
}
impl From<LxPaymentSecret> for PaymentSecret {
    fn from(secret: LxPaymentSecret) -> Self {
        Self(secret.0)
    }
}

// As recommended by LDK, we use LxPaymentHash as our PaymentId
impl From<PaymentId> for LxPaymentHash {
    fn from(id: PaymentId) -> Self {
        Self(id.0)
    }
}
impl From<LxPaymentHash> for PaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
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
