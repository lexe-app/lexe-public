use serde::{Deserialize, Serialize};

use crate::payments::offchain::inbound::{
    InboundInvoicePayment, InboundSpontaneousPayment,
};
use crate::payments::offchain::outbound::{
    OutboundInvoicePayment, OutboundSpontaneousPayment,
};
use crate::payments::LxPaymentHash;

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
