use std::time::Duration;

use anyhow::{bail, ensure};
use common::ln::amount::Amount;
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
    /// The amount sent in this payment, given by [`Route::get_total_amount`].
    pub amount: Amount,
    /// The routing fees for this payment. If the payment hasn't completed yet,
    /// this value is only an estimation based on a [`Route`] computed prior to
    /// the first send attempt, as the actual fees paid may vary somewhat due
    /// to retries occurring on different paths. If the payment is
    /// completed, then this field should reflect the actual fees paid.
    pub fees: Amount,
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
    /// The invoice expired and we called [`ChannelManager::abandon_payment`],
    /// but we haven't yet received a [`PaymentFailed`] (or [`PaymentSent`])
    /// event to finalize the payment.
    ///
    /// This state is "pending" (and not "finalized") because calling
    /// `abandon_payment` does not actually prevent the payment from
    /// succeeding. See the `abandon_payment` docs for more details.
    Abandoning,
    /// We received a [`PaymentSent`] event.
    Completed,
    /// We received a [`PaymentFailed`] event, or the initial send in
    /// [`pay_invoice`] "failed outright".
    // TODO(max): Reject the payment of invoices which have timed out
    Failed,
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
            amount: Amount::from_msat(route.get_total_amount()),
            fees: Amount::from_msat(route.get_total_fees()),
            status: OutboundInvoicePaymentStatus::Pending,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    pub(crate) fn check_payment_sent(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid: Option<Amount>,
    ) -> anyhow::Result<Self> {
        use OutboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");

        let computed_hash = preimage.compute_hash();
        ensure!(hash == computed_hash, "Preimage doesn't correspond to hash");

        let estimated_fees = &self.fees;
        let final_fees = maybe_fees_paid
            .inspect(|fees_paid| {
                if fees_paid != estimated_fees {
                    info!(
                        %hash,
                        "Estimated fees from Route was {estimated_fees} msat; \
                        actually paid {fees_paid} msat."
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
            Abandoning => warn!(
                %hash,
                "Attempted to abandon this OIP but it succeeded anyway",
            ),
            Completed | Failed => bail!("OIP was already finel"),
        }

        let mut clone = self.clone();
        clone.preimage = Some(preimage);
        clone.fees = final_fees;
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
            Pending | Abandoning => (),
            Completed | Failed => bail!("OIP was already final"),
        }

        let mut clone = self.clone();
        clone.status = Failed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Checks whether this payment's invoice has expired. If so, and if the
    /// state transition to `Abandoning` is valid, returns a clone with the
    /// state transition applied.
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
            // Since Abandoning is a pending state, the invoice expiry checker
            // will frequently check already-abandoning payments to see if they
            // have expired. To prevent the PaymentsManager from constantly
            // re-persisting already-abandoning payments during these checks,
            // return None here.
            Abandoning => return None,
            Completed | Failed => return None,
        }

        // Validation complete; invoice expired and state transition is valid

        let mut clone = self.clone();
        clone.status = Abandoning;

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
    /// The amount received in this payment.
    pub amount: Amount,
    /// The fees we paid for this payment, given by [`Route::get_total_fees`].
    pub fees: Amount,
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
