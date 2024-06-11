use std::time::Duration;

use anyhow::{bail, ensure};
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    ln::{
        amount::Amount,
        invoice::LxInvoice,
        payments::{
            LxPaymentHash, LxPaymentId, LxPaymentPreimage, LxPaymentSecret,
        },
    },
    time::TimestampMs,
};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::Event::{PaymentFailed, PaymentSent},
    events::PaymentPurpose,
    ln::channelmanager::ChannelManager,
};
use lightning::{
    events::PaymentFailureReason, ln::channelmanager::Retry,
    routing::router::Route,
};
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
    /// For a failed payment, the reason why it failed.
    pub failure: Option<LxOutboundPaymentFailure>,
    /// An optional personal note for this payment. Since the receiver sets the
    /// invoice description, which might just be an unhelpful 🍆 emoji, the
    /// user has the option to add this note at the time of invoice
    /// payment.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
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
    pub fn new(
        invoice: LxInvoice,
        route: &Route,
        note: Option<String>,
    ) -> Self {
        let hash = invoice.payment_hash();
        let secret = invoice.payment_secret();
        Self {
            invoice: Box::new(invoice),
            hash,
            secret,
            preimage: None,
            amount: Amount::from_msat(route.get_total_amount()),
            fees: Amount::from_msat(route.get_total_fees()),
            status: OutboundInvoicePaymentStatus::Pending,
            failure: None,
            note,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
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
        failure: LxOutboundPaymentFailure,
    ) -> anyhow::Result<Self> {
        use OutboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");

        match self.status {
            Pending | Abandoning => (),
            Completed | Failed => bail!("OIP was already final"),
        }

        let mut clone = self.clone();
        clone.status = Failed;
        clone.failure = Some(failure);
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
    /// An optional personal note for this payment. Since there is no invoice
    /// description field, the user has the option to set this at payment
    /// creation time.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

impl OutboundSpontaneousPayment {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }
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

// --- Outbound Payment Failure --- //

/// Contains a reason for why an outbound lightning payment failed.
///
/// Unfortunately, LDK's current error messages (via event handling) are not
/// particularly helpful -- all the useful info is emitted via the LDK logger.
/// But this is still better than just seeing "Failed failed" in the UI.
///
/// See: [`lightning::events::PaymentFailureReason`]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum LxOutboundPaymentFailure {
    /// We exhausted all of our retry attempts.
    NoRetries,
    /// The intended recipient rejected our payment.
    Rejected,
    /// The user abandoned this payment via `ChannelManager::abandon_payment`.
    Abandoned,
    /// The payment expired while retrying.
    Expired,
    /// Failed to route the payment while retrying.
    NoRoute,
    /// API misuse error. Probably a bug in Lexe code.
    LexeErr,
    /// Any unrecognized variant we might deserialize. This variant is for
    /// forwards compatibility (old node reads new state).
    #[serde(other)]
    Unknown,
}

impl LxOutboundPaymentFailure {
    // TODO(phlip9): generate this programmatically
    #[cfg(test)]
    const VARIANTS: [Self; 7] = [
        Self::NoRetries,
        Self::Rejected,
        Self::Abandoned,
        Self::Expired,
        Self::NoRoute,
        Self::LexeErr,
        Self::Unknown,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoRetries => "no successful payment after all retry attempts",
            Self::Rejected => "the recipient rejected our payment",
            Self::Abandoned => "the payment was canceled",
            Self::Expired =>
                "the invoice expired before we could complete the payment",
            Self::NoRoute => "could not find usable route to send payment over",
            Self::LexeErr => "probable bug in LEXE user node payment router",
            Self::Unknown => "unknown error, app is likely out-of-date",
        }
    }
}

impl From<PaymentFailureReason> for LxOutboundPaymentFailure {
    fn from(value: PaymentFailureReason) -> Self {
        use PaymentFailureReason::*;
        match value {
            RecipientRejected => Self::Rejected,
            UserAbandoned => Self::Abandoned,
            RetriesExhausted => Self::NoRetries,
            PaymentExpired => Self::Expired,
            RouteNotFound => Self::NoRoute,
            UnexpectedError => Self::LexeErr,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // TODO(phlip9): if we can get a VariantArray trait or smth, then we can
    // generalize this test.
    #[test]
    fn lx_outbound_payment_failure_json_backward_compat() {
        // Pin the serialization for backward compatibility
        let expected_de = LxOutboundPaymentFailure::VARIANTS.to_vec();
        let expected_ser = "[\"NoRetries\",\"Rejected\",\"Abandoned\",\"Expired\",\"NoRoute\",\"LexeErr\",\"Unknown\"]";
        let actual_ser = serde_json::to_string(&expected_de).unwrap();
        let actual_de =
            serde_json::from_str::<Vec<LxOutboundPaymentFailure>>(expected_ser)
                .unwrap();
        assert_eq!(actual_ser, expected_ser);
        assert_eq!(actual_de, expected_de);
    }

    #[test]
    fn lx_outbound_payment_failure_json_forward_compat() {
        let s = "\"SomeNewVariant\"";
        let expected_de = LxOutboundPaymentFailure::Unknown;
        let actual_de =
            serde_json::from_str::<LxOutboundPaymentFailure>(s).unwrap();
        assert_eq!(actual_de, expected_de);
    }
}
