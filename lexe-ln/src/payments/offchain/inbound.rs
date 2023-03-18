use anyhow::{bail, ensure, Context};
use common::ln::invoice::LxInvoice;
use common::time::TimestampMillis;
#[cfg(doc)]
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::util::events::Event::{PaymentClaimable, PaymentClaimed};
use lightning::util::events::PaymentPurpose;
use serde::{Deserialize, Serialize};
use tracing::warn;

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

impl InboundInvoicePayment {
    pub fn new(
        invoice: LxInvoice,
        hash: PaymentHash,
        secret: PaymentSecret,
        preimage: PaymentPreimage,
    ) -> Self {
        let invoice_amt_msat = invoice.0.amount_milli_satoshis();
        Self {
            invoice: Box::new(invoice),
            hash: LxPaymentHash::from(hash),
            secret: LxPaymentSecret::from(secret),
            preimage: LxPaymentPreimage::from(preimage),
            invoice_amt_msat,
            recvd_amount_msat: None,
            onchain_fees_msat: None,
            status: InboundInvoicePaymentStatus::InvoiceGenerated,
            created_at: TimestampMillis::now(),
            finalized_at: None,
        }
    }

    pub fn payment_claimable(
        &mut self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        use InboundInvoicePaymentStatus as Status;
        match self.status {
            Status::InvoiceGenerated => (),
            Status::Claiming => warn!("Re-claiming inbound invoice payment"),
            Status::Completed | Status::TimedOut => {
                bail!("Payment already final")
            }
        }

        ensure!(hash == self.hash, "Hashes don't match");

        match purpose {
            PaymentPurpose::InvoicePayment {
                payment_preimage,
                payment_secret,
            } => {
                let given_preimage =
                    payment_preimage.map(LxPaymentPreimage::from).context(
                        "We previously generated this invoice using a method \
                        other than `ChannelManager::create_inbound_payment`, \
                        resulting in the channel manager not being aware of \
                        the payment preimage, OR LDK failed to provide the \
                        preimage back to us.",
                    )?;
                let given_secret = LxPaymentSecret::from(payment_secret);
                ensure!(
                    given_preimage == self.preimage,
                    "Preimages don't match",
                );
                ensure!(given_secret == self.secret, "Secrets don't match");
            }
            PaymentPurpose::SpontaneousPayment { .. } => {
                bail!("This is not a spontaneous payment")
            }
        };

        if let Some(invoice_amt_msat) = self.invoice_amt_msat {
            if amt_msat < invoice_amt_msat {
                warn!("Requested {invoice_amt_msat} but claiming {amt_msat}");
                // TODO(max): In the future, we might want to bail! instead
            }
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok, update our state
        self.recvd_amount_msat = Some(amt_msat);
        self.status = InboundInvoicePaymentStatus::Claiming;

        Ok(())
    }
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

impl InboundSpontaneousPayment {
    pub fn payment_claimable(
        &mut self,
        _payment_hash: LxPaymentHash,
        _amount_msat: u64,
        _purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
