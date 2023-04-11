use common::ln::invoice::LxInvoice;
use common::ln::payments::{LxPaymentHash, LxPaymentPreimage, LxPaymentSecret};
use common::time::TimestampMs;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
use lightning::routing::router::Route;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{PaymentFailed, PaymentSent};
#[cfg(doc)]
use lightning::util::events::PaymentPurpose;
use lightning_invoice::Invoice;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::command::pay_invoice;

// --- Outbound invoice payments --- //

/// A 'conventional' outbound payment where we pay an invoice provided to us by
/// our recipient.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OutboundInvoicePayment {
    /// The invoice given by our recipient which we want to pay.
    // LxInvoice is ~300 bytes, Box to avoid the enum variant lint
    pub invoice: Box<LxInvoice>,
    /// The payment hash encoded in the invoice.
    pub hash: LxPaymentHash,
    /// The payment secret encoded in the invoice.
    // BOLT11: "A writer: [...] MUST include exactly one `s` field."
    pub secret: LxPaymentSecret,
    /// The preimage, which serves as a proof-of-payment.
    /// This field is populated if and only if the status is `Completed`.
    pub preimage: Option<LxPaymentPreimage>,
    /// The millisat amount sent in this payment,
    /// given by [`Route::get_total_amount`].
    // TODO(max): Use LDK-provided Amount newtype when available
    pub amt_msat: u64,
    /// The fees we paid for this payment, given by [`Route::get_total_fees`].
    // TODO(max): Use LDK-provided Amount newtype when available
    pub fees_msat: u64,
    /// The current status of the payment.
    pub status: OutboundInvoicePaymentStatus,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OutboundInvoicePaymentStatus {
    /// We initiated the payment with [`pay_invoice`].
    Pending,
    /// We received a [`PaymentSent`] event.
    Completed,
    /// We received a [`PaymentFailed`] event.
    // TODO(max): Reject the payment of invoices which have timed out
    Failed,
    /// The invoice we want to pay has expired, and we called
    /// [`ChannelManager::abandon_payment`]
    TimedOut,
}

impl OutboundInvoicePayment {
    pub fn new(invoice: Invoice, route: &Route) -> Self {
        let hash = LxPaymentHash::from(*invoice.payment_hash());
        let secret = LxPaymentSecret::from(*invoice.payment_secret());
        Self {
            invoice: Box::new(LxInvoice(invoice)),
            hash,
            secret,
            preimage: None,
            amt_msat: route.get_total_amount(),
            fees_msat: route.get_total_fees(),
            status: OutboundInvoicePaymentStatus::Pending,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }
}

// --- Outbound spontaneous payments --- //

/// An outbound spontaneous (`keysend`) payment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OutboundSpontaneousPayment {
    /// The hash of this payment.
    pub hash: LxPaymentHash,
    /// The preimage used in this payment, which is generated by us, must match
    /// the hash of this payment, and which must be globally unique to ensure
    /// that intermediate nodes cannot steal funds.
    pub preimage: LxPaymentPreimage,
    /// The millisat amount received in this payment.
    // TODO(max): Use LDK-provided Amount newtype when available
    pub amt_msat: u64,
    /// The fees we paid for this payment, given by [`Route::get_total_fees`].
    // TODO(max): Use LDK-provided Amount newtype when available
    pub fees_msat: u64,
    /// The current status of the payment.
    pub status: OutboundSpontaneousPaymentStatus,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum OutboundSpontaneousPaymentStatus {
    /// We initiated the payment with `send_spontaneous_payment`.
    // TODO(max): Actually implement sending spontaneous payments
    Pending,
    /// We received a [`PaymentSent`] event.
    Completed,
    /// We received a [`PaymentFailed`] event.
    Failed,
}
