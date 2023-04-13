use std::time::Duration;

use anyhow::{bail, ensure};
use common::ln::invoice::LxInvoice;
use common::ln::payments::{LxPaymentHash, LxPaymentPreimage, LxPaymentSecret};
use common::time::TimestampMs;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::channelmanager::Retry;
use lightning::routing::router::Route;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{PaymentFailed, PaymentSent};
#[cfg(doc)]
use lightning::util::events::PaymentPurpose;
use lightning_invoice::Invoice;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[cfg(doc)]
use crate::command::pay_invoice;

/// The retry strategy we pass to LDK for outbound Lightning payments.
pub const OUTBOUND_PAYMENT_RETRY_STRATEGY: Retry = Retry::Attempts(3);

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
    /// The routing fees for this payment. If the payment hasn't completed yet,
    /// this value is only an estimation based on a [`Route`] computed prior to
    /// the first send attempt, as the actual fees paid may vary somewhat due
    /// to retries occurring on different paths. If the payment is
    /// completed, then this field should reflect the actual fees paid.
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

    pub(crate) fn check_payment_sent(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid_msat: Option<u64>,
    ) -> anyhow::Result<Self> {
        use OutboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");

        let computed_hash = preimage.compute_hash();
        ensure!(hash == computed_hash, "Preimage doesn't correspond to hash");

        let estimated_fees = &self.fees_msat;
        let final_fees_msat = maybe_fees_paid_msat
            .inspect(|fees_paid_msat| {
                if fees_paid_msat != estimated_fees {
                    info!(
                        %hash,
                        "Estimated fees from Route was {estimated_fees} msat; \
                        actually paid {fees_paid_msat} msat."
                    );
                }
            })
            .unwrap_or_else(|| {
                warn!(
                    "Did not hear back on final fees paid for OIP; the \
                    estimated fee will be included with the finalized payment."
                );
                *estimated_fees
            });

        match self.status {
            Pending => (),
            // TODO(max): When we implement timeouts, we need to ensure that we
            // have told our ChannelManager to cancel trying to send the payment
            // before we actually update the payment to TimedOut. This ensures
            // that once our payment has been "finalized" via TimedOut, it stays
            // finalized.
            Completed | Failed | TimedOut => bail!("OIP was already finel"),
        }

        let mut clone = self.clone();
        clone.preimage = Some(preimage);
        clone.fees_msat = final_fees_msat;
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    pub(crate) fn check_payment_failed(
        &self,
        hash: LxPaymentHash,
    ) -> anyhow::Result<Self> {
        use OutboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");

        match self.status {
            Pending => (),
            Completed | Failed | TimedOut => bail!("OIP was already final"),
        }

        let mut clone = self.clone();
        clone.status = Failed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Checks whether this payment's invoice has expired. If so, and if the
    /// state transition to `TimedOut` is valid, returns a clone with the state
    /// transition applied.
    ///
    /// `unix_duration` is the current time expressed as a [`Duration`] since
    /// the unix epoch.
    pub(crate) fn check_invoice_expiry(
        &self,
        unix_duration: Duration,
    ) -> Option<Self> {
        use OutboundInvoicePaymentStatus::*;

        if !self.invoice.0.would_expire(unix_duration) {
            return None;
        }

        match self.status {
            Pending => (),
            Completed | Failed | TimedOut => return None,
        }

        // Validation complete; invoice expired and TimedOut transition is valid

        let mut clone = self.clone();
        clone.status = TimedOut;
        clone.finalized_at = Some(TimestampMs::now());

        Some(clone)
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
