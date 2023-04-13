use std::convert::TryFrom;
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use common::ln::invoice::LxInvoice;
use common::ln::payments::{LxPaymentHash, LxPaymentPreimage, LxPaymentSecret};
use common::time::TimestampMs;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{PaymentClaimable, PaymentClaimed};
use lightning::util::events::PaymentPurpose;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(doc)]
use crate::command::create_invoice;
use crate::payments::manager::CheckedPayment;
use crate::payments::Payment;

// --- LxPaymentPurpose --- //

/// A newtype for [`PaymentPurpose`] which (1) gets rid of the repetitive
/// `.context()?`s on the `InvoicePayment::payment_preimage` field via a
/// [`TryFrom`] impl, (2) converts LDK payment types to Lexe payment types, and
/// (3) exposes a [`preimage`] method to avoid yet another unnecessary match.
///
/// [`preimage`]: Self::preimage
#[derive(Copy, Clone)]
pub enum LxPaymentPurpose {
    Invoice {
        preimage: LxPaymentPreimage,
        secret: LxPaymentSecret,
    },
    Spontaneous {
        preimage: LxPaymentPreimage,
    },
}

impl LxPaymentPurpose {
    pub fn preimage(&self) -> LxPaymentPreimage {
        match self {
            Self::Invoice { preimage, .. } => *preimage,
            Self::Spontaneous { preimage } => *preimage,
        }
    }
}

impl TryFrom<PaymentPurpose> for LxPaymentPurpose {
    type Error = anyhow::Error;
    fn try_from(purpose: PaymentPurpose) -> anyhow::Result<Self> {
        match purpose {
            PaymentPurpose::InvoicePayment {
                payment_preimage,
                payment_secret,
            } => {
                let preimage =
                    payment_preimage.map(LxPaymentPreimage::from).context(
                        "We previously generated this invoice using a method \
                        other than `ChannelManager::create_inbound_payment`, \
                        OR LDK failed to provide the preimage back to us.",
                    )?;
                let secret = LxPaymentSecret::from(payment_secret);
                Ok(Self::Invoice { preimage, secret })
            }
            PaymentPurpose::SpontaneousPayment(payment_preimage) => {
                let preimage = LxPaymentPreimage::from(payment_preimage);
                Ok(Self::Spontaneous { preimage })
            }
        }
    }
}

// --- Helpers to delegate to the inner type --- //

/// Helper to handle the [`Payment`] and [`LxPaymentPurpose`] matching.
// Normally we don't want this much indirection, but the calling code is already
// doing lots of ugly matching (at a higher abstraction level), so in this case
// the separation makes both functions cleaner and easier to read.
impl Payment {
    pub(crate) fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: LxPaymentPurpose,
    ) -> anyhow::Result<CheckedPayment> {
        match (self, purpose) {
            (
                Self::InboundInvoice(iip),
                LxPaymentPurpose::Invoice { preimage, secret },
            ) => iip
                .check_payment_claimable(hash, secret, preimage, amt_msat)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error claiming inbound invoice payment"),
            (
                Self::InboundSpontaneous(isp),
                LxPaymentPurpose::Spontaneous { preimage },
            ) => isp
                .check_payment_claimable(hash, preimage, amt_msat)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error claiming inbound spontaneous payment"),
            _ => bail!("Not an inbound LN payment, or purpose didn't match"),
        }
    }
}

// --- Inbound invoice payments --- //

/// A 'conventional' inbound payment which is facilitated by an invoice.
/// This struct is created when we call [`create_invoice`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct InboundInvoicePayment {
    /// Created in [`create_invoice`].
    // LxInvoice is ~300 bytes, Box to avoid the enum variant lint
    pub invoice: Box<LxInvoice>,
    /// Returned by [`ChannelManager::create_inbound_payment`] inside
    /// [`create_invoice`].
    pub hash: LxPaymentHash,
    /// Returned by [`ChannelManager::create_inbound_payment`] inside
    /// [`create_invoice`].
    pub secret: LxPaymentSecret,
    /// Returned by:
    /// - the call to [`ChannelManager::get_payment_preimage`] inside
    ///   [`create_invoice`].
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
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `TimedOut`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
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

impl InboundInvoicePayment {
    pub fn new(
        invoice: LxInvoice,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
    ) -> Self {
        let invoice_amt_msat = invoice.0.amount_milli_satoshis();
        Self {
            invoice: Box::new(invoice),
            hash,
            secret,
            preimage,
            invoice_amt_msat,
            recvd_amount_msat: None,
            onchain_fees_msat: None,
            status: InboundInvoicePaymentStatus::InvoiceGenerated,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
        amt_msat: u64,
    ) -> anyhow::Result<Self> {
        use InboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(secret == self.secret, "Secrets don't match");

        match self.status {
            InvoiceGenerated => (),
            Claiming => warn!("Re-claiming inbound invoice payment"),
            Completed | TimedOut => {
                bail!("Payment already final")
            }
        }

        if let Some(invoice_amt_msat) = self.invoice_amt_msat {
            if amt_msat < invoice_amt_msat {
                warn!("Requested {invoice_amt_msat} but claiming {amt_msat}");
                // TODO(max): In the future, we might want to bail! instead
            }
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.recvd_amount_msat = Some(amt_msat);
        clone.status = InboundInvoicePaymentStatus::Claiming;

        Ok(clone)
    }

    pub(crate) fn check_payment_claimed(
        &self,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
        amt_msat: u64,
    ) -> anyhow::Result<Self> {
        use InboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(secret == self.secret, "Secrets don't match");

        match self.status {
            InvoiceGenerated => {
                // We got PaymentClaimed without PaymentClaimable, which should
                // be rare because it requires a channel manager persist race.
                warn!(
                    "Inbound invoice payment was claimed without a \
                      corresponding PaymentClaimable event"
                );
            }
            Claiming => (),
            Completed => {
                // We will never claim the same payment twice, so LDK's docs on
                // PaymentClaimed don't apply here.
                bail!("Payment already claimed")
            }
            TimedOut => bail!("Payment already timed out"),
        }

        if let Some(invoice_amt_msat) = self.invoice_amt_msat {
            if amt_msat < invoice_amt_msat {
                warn!("Requested {invoice_amt_msat} but claimed {amt_msat}");
                // TODO(max): In the future, we might want to bail! instead
            }
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.recvd_amount_msat = Some(amt_msat);
        clone.status = Completed;
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
        use InboundInvoicePaymentStatus::*;

        if !self.invoice.0.would_expire(unix_duration) {
            return None;
        }

        match self.status {
            InvoiceGenerated => (),
            // We are already claiming the payment; too late to time it out now.
            Claiming => return None,
            // Don't time out finalized payments.
            Completed | TimedOut => return None,
        }

        // Validation complete; invoice expired and TimedOut transition is valid

        let mut clone = self.clone();
        clone.status = TimedOut;
        clone.finalized_at = Some(TimestampMs::now());

        Some(clone)
    }
}

// --- Inbound spontaneous payments --- //

/// An inbound spontaneous (`keysend`) payment. This struct is created when we
/// get a [`PaymentClaimable`] event, where the [`PaymentPurpose`] is of the
/// `SpontaneousPayment` variant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
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
    pub created_at: TimestampMs,
    /// When this payment reached the `Completed` state.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
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

impl InboundSpontaneousPayment {
    pub(crate) fn new(
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amt_msat: u64,
    ) -> Self {
        Self {
            hash,
            preimage,
            amt_msat,
            // TODO(max): Implement
            onchain_fees_msat: None,
            status: InboundSpontaneousPaymentStatus::Claiming,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amt_msat: u64,
    ) -> anyhow::Result<Self> {
        use InboundSpontaneousPaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(amt_msat == self.amt_msat, "Amounts don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(matches!(self.status, Claiming), "Payment already finalized");

        // We handled the PaymentClaimable event twice, which should only happen
        // rarely (requires persistence race). Log a warning to make some noise.
        warn!("Reclaiming existing spontaneous payment");

        // There is no state to update, just return a clone of self.
        Ok(self.clone())
    }

    pub(crate) fn check_payment_claimed(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amt_msat: u64,
    ) -> anyhow::Result<Self> {
        use InboundSpontaneousPaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(amt_msat == self.amt_msat, "Amounts don't match");

        match self.status {
            Claiming => (),
            Completed => bail!("Payment already claimed"),
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }
}
