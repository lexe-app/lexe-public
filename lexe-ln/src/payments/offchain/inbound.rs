use common::ln::invoice::LxInvoice;
use common::time::TimestampMillis;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{PaymentClaimable, PaymentClaimed};
#[cfg(doc)]
use lightning::util::events::PaymentPurpose;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::command::get_invoice;
use crate::payments::{LxPaymentHash, LxPaymentPreimage, LxPaymentSecret};

// --- Inbound invoice payments --- //

/// A 'conventional' inbound payment which is facilitated by an invoice.
/// This struct is created when we call [`get_invoice`].
#[derive(Clone, Serialize, Deserialize)]
pub struct InboundInvoicePayment {
    /// Created in [`get_invoice`].
    // LxInvoice is ~300 bytes, Box to avoid the enum variant lint
    pub invoice: Box<LxInvoice>,
    /// Returned by [`ChannelManager::create_inbound_payment`] inside
    /// [`get_invoice`].
    pub hash: LxPaymentHash,
    /// Returned by [`ChannelManager::create_inbound_payment`] inside
    /// [`get_invoice`].
    pub secret: LxPaymentSecret,
    /// Returned by:
    /// - the call to [`ChannelManager::get_payment_preimage`] inside
    ///   [`get_invoice`].
    /// - the [`PaymentPurpose`] field of the [`PaymentClaimable`] event.
    /// - the [`PaymentPurpose`] field of the [`PaymentClaimed`] event.
    pub preimage: LxPaymentPreimage,
    /// The millisat amount encoded in our invoice, if there was one.
    // TODO(max): Use LDK-provided Amount newtype when available
    pub invoice_amt_msat: Option<u64>,
    /// The millisat amount that we actually received.
    /// Populated iff we received a [`PaymentClaimable`] event.
    // TODO(max): Use LDK-provided Amount newtype when available
    pub recvd_amount_msat: Option<u64>,
    /// The millisat amount we paid in on-chain fees (possibly arising from
    /// receiving our payment over a JIT channel) to receive this transaction.
    // TODO(max): Implement
    // TODO(max): Use LDK-provided Amount newtype when available
    pub onchain_fees_msat: Option<u64>,
    /// The current status of the payment.
    pub status: InboundInvoicePaymentStatus,
    /// When we created the invoice for this payment.
    pub created_at: TimestampMillis,
    /// When this payment either `Completed` or `TimedOut`.
    pub finalized_at: Option<TimestampMillis>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum InboundInvoicePaymentStatus {
    /// We generated an invoice, but it hasn't been paid yet.
    InvoiceGenerated,
    /// We are currently claiming the payment, i.e. we received a
    /// [`PaymentClaimable`] event.
    Claiming,
    /// The inbound payment has been completed, i.e. we received a
    /// [`PaymentClaimed`] event.
    Completed,
    /// The inbound payment has reached its invoice expiry time. Any
    /// [`PaymentClaimable`] events which appear after this should be rejected.
    // TODO(max): Implement automatic timeout of generated invoices.
    // TODO(max): Reject any PaymentClaimable events for timed out payments.
    TimedOut,
}

// --- Inbound spontaneous payments --- //

/// An inbound spontaneous (`keysend`) payment. This struct is created when we
/// get a [`PaymentClaimable`] event, where the [`PaymentPurpose`] is of the
/// `SpontaneousPayment` variant.
#[derive(Clone, Serialize, Deserialize)]
pub struct InboundSpontaneousPayment {
    /// Given by [`PaymentClaimable`] and [`PaymentClaimed`].
    pub hash: LxPaymentHash,
    /// Given by [`PaymentPurpose`].
    pub preimage: LxPaymentPreimage,
    /// The millisat amount received in this payment.
    // TODO(max): Use LDK-provided Amount newtype when available
    pub amt_msat: u64,
    /// The millisat amount we paid in on-chain fees (possibly arising from
    /// receiving our payment over a JIT channel) to receive this transaction.
    // TODO(max): Implement
    // TODO(max): Use LDK-provided Amount newtype when available
    pub onchain_fees_msat: Option<u64>,
    /// The current status of the payment.
    pub status: InboundSpontaneousPaymentStatus,
    /// When we first learned of this payment via [`PaymentClaimable`].
    pub created_at: TimestampMillis,
    /// When this payment reached the `Completed` state.
    pub finalized_at: Option<TimestampMillis>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum InboundSpontaneousPaymentStatus {
    /// We received a [`PaymentClaimable`] event.
    Claiming,
    /// We received a [`PaymentClaimed`] event.
    Completed,
    // NOTE: We don't have a "Failed" case here because (as Matt says) if you
    // call ChannelManager::claim_funds we should always get the
    // PaymentClaimed event back. If for some reason this turns out not to
    // be true (i.e. we observe a number of inbound spontaneous payments
    // stuck in the "claiming" state), then we can add a "Failed" state
    // here. https://discord.com/channels/915026692102316113/978829624635195422/1085427776070365214
}
