use std::time::Duration;

use anyhow::{bail, ensure, Context};
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    ln::{
        amount::Amount,
        invoice::LxInvoice,
        payments::{
            LxPaymentHash, LxPaymentId, LxPaymentPreimage, LxPaymentSecret,
            PaymentKind,
        },
    },
    time::TimestampMs,
};
use lightning::{
    blinded_path::payment::{Bolt12OfferContext, Bolt12RefundContext},
    events::PaymentPurpose,
};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::Event::{PaymentClaimable, PaymentClaimed},
    ln::channelmanager::ChannelManager,
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(doc)]
use crate::command::create_invoice;
use crate::payments::{manager::CheckedPayment, Payment};

// --- LxPaymentPurpose --- //

/// A newtype for [`PaymentPurpose`] which
///
/// 1) Changes the BOLT 11 [`payment_preimage`] field to be non-[`Option`] given
///    we expect the field to always be [`Some`] (which is because we want LDK
///    to handle preimages for us by using [`create_inbound_payment`] instead of
///    [`create_inbound_payment_for_hash`]).
/// 2) Converts LDK payment types to Lexe payment types.
/// 3) Exposes a convenience method to get the contained [`LxPaymentPreimage`].
///
/// [`payment_preimage`]: lightning::events::PaymentPurpose::Bolt11InvoicePayment::payment_preimage
/// [`create_inbound_payment`]: lightning::ln::channelmanager::ChannelManager::create_inbound_payment
/// [`create_inbound_payment_for_hash`]: lightning::ln::channelmanager::ChannelManager::create_inbound_payment_for_hash
#[derive(Clone)]
pub enum LxPaymentPurpose {
    Bolt11Invoice {
        preimage: LxPaymentPreimage,
        secret: LxPaymentSecret,
    },
    Bolt12Offer {
        preimage: LxPaymentPreimage,
        secret: LxPaymentSecret,
        context: Bolt12OfferContext,
    },
    Bolt12Refund {
        preimage: LxPaymentPreimage,
        secret: LxPaymentSecret,
        context: Bolt12RefundContext,
    },
    Spontaneous {
        preimage: LxPaymentPreimage,
    },
}

impl LxPaymentPurpose {
    pub fn preimage(&self) -> LxPaymentPreimage {
        match self {
            Self::Bolt11Invoice { preimage, .. } => *preimage,
            Self::Bolt12Offer { preimage, .. } => *preimage,
            Self::Bolt12Refund { preimage, .. } => *preimage,
            Self::Spontaneous { preimage } => *preimage,
        }
    }

    /// Get the [`PaymentKind`] which corresponds to this [`LxPaymentPurpose`].
    pub fn kind(&self) -> PaymentKind {
        // TODO(max): Implement for BOLT 12
        match self {
            Self::Bolt11Invoice { .. } => PaymentKind::Invoice,
            Self::Bolt12Offer { .. } => todo!("Not sure of new variant yet"),
            Self::Bolt12Refund { .. } => todo!("Not sure of new variant yet"),
            Self::Spontaneous { .. } => PaymentKind::Spontaneous,
        }
    }
}

impl TryFrom<PaymentPurpose> for LxPaymentPurpose {
    type Error = anyhow::Error;
    fn try_from(purpose: PaymentPurpose) -> anyhow::Result<Self> {
        let no_preimage_msg =
            "We should always let LDK handle payment preimages for us by \
             always using `ChannelManager::create_inbound_payment` instead of \
             `ChannelManager::create_inbound_payment_for_hash`. \
             Either we failed to do this, or there is a bug in LDK.";
        let maybe_preimage = purpose.preimage().map(LxPaymentPreimage::from);
        debug_assert!(maybe_preimage.is_some(), "{no_preimage_msg}");
        let preimage = maybe_preimage.context(no_preimage_msg)?;

        match purpose {
            PaymentPurpose::Bolt11InvoicePayment {
                payment_preimage: _,
                payment_secret,
            } => {
                let secret = LxPaymentSecret::from(payment_secret);
                Ok(Self::Bolt11Invoice { preimage, secret })
            }
            PaymentPurpose::Bolt12OfferPayment {
                payment_preimage: _,
                payment_secret,
                payment_context: context,
            } => {
                let secret = LxPaymentSecret::from(payment_secret);
                Ok(Self::Bolt12Offer {
                    preimage,
                    secret,
                    context,
                })
            }
            PaymentPurpose::Bolt12RefundPayment {
                payment_preimage: _,
                payment_secret,
                payment_context: context,
            } => {
                let secret = LxPaymentSecret::from(payment_secret);
                Ok(Self::Bolt12Refund {
                    preimage,
                    secret,
                    context,
                })
            }
            PaymentPurpose::SpontaneousPayment(_payment_preimage) =>
                Ok(Self::Spontaneous { preimage }),
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
        amount: Amount,
        purpose: LxPaymentPurpose,
    ) -> anyhow::Result<CheckedPayment> {
        // TODO(max): Update this

        ensure!(
            purpose.kind() == self.kind(),
            "Purpose kind doesn't match payment kind: {purkind} != {paykind}",
            purkind = purpose.kind(),
            paykind = self.kind(),
        );

        match (self, purpose) {
            (
                Self::InboundInvoice(iip),
                LxPaymentPurpose::Bolt11Invoice { preimage, secret },
            ) => iip
                .check_payment_claimable(hash, secret, preimage, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error claiming inbound invoice payment"),
            // TODO(max): Implement for BOLT 12
            // (
            //     Self::Bolt12Offer(b12o),
            //     LxPaymentPurpose::Bolt12Offer {
            //         preimage,
            //         secret,
            //         context,
            //     },
            // ) => {
            //     let _ = preimage;
            //     let _ = secret;
            //     let _ = context;
            //     todo!();
            // }
            // (
            //     Self::Bolt12Refund(b12r),
            //     LxPaymentPurpose::Bolt12Refund {
            //         preimage,
            //         secret,
            //         context,
            //     },
            // ) => {
            //     let _ = preimage;
            //     let _ = secret;
            //     let _ = context;
            //     todo!();
            // }
            (
                Self::InboundSpontaneous(isp),
                LxPaymentPurpose::Spontaneous { preimage },
            ) => isp
                .check_payment_claimable(hash, preimage, amount)
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
    /// The amount encoded in our invoice, if there was one.
    pub invoice_amount: Option<Amount>,
    /// The amount that we actually received.
    /// Populated iff we received a [`PaymentClaimable`] event.
    pub recvd_amount: Option<Amount>,
    /// The amount we paid in on-chain fees (possibly arising from receiving
    /// our payment over a JIT channel) to receive this transaction.
    // TODO(max): Implement
    pub onchain_fees: Option<Amount>,
    /// The current status of the payment.
    pub status: InboundInvoicePaymentStatus,
    /// An optional personal note for this payment. Since a user-provided
    /// description is already required when creating an invoice, at invoice
    /// creation time this field is not exposed to the user and is simply
    /// initialized to [`None`]. Useful primarily if a user wants to update
    /// their note later.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    /// When we created the invoice for this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Expired`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
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
    Expired,
}

impl InboundInvoicePayment {
    pub fn new(
        invoice: LxInvoice,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
    ) -> Self {
        let invoice_amount =
            invoice.0.amount_milli_satoshis().map(Amount::from_msat);
        Self {
            invoice: Box::new(invoice),
            hash,
            secret,
            preimage,
            invoice_amount,
            recvd_amount: None,
            onchain_fees: None,
            status: InboundInvoicePaymentStatus::InvoiceGenerated,
            note: None,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }

    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
        amount: Amount,
    ) -> anyhow::Result<Self> {
        use InboundInvoicePaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(secret == self.secret, "Secrets don't match");
        // BOLT11: "A payee: after the timestamp plus expiry has passed: SHOULD
        // NOT accept a payment."
        ensure!(!self.invoice.0.is_expired(), "Invoice has already expired");

        match self.status {
            InvoiceGenerated => (),
            Claiming => warn!("Re-claiming inbound invoice payment"),
            Completed | Expired => {
                bail!("Payment already final")
            }
        }

        if let Some(invoice_amount) = self.invoice_amount {
            if amount < invoice_amount {
                warn!("Requested {invoice_amount} but claiming {amount}");
                // TODO(max): In the future, we might want to bail! instead
            }
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.recvd_amount = Some(amount);
        clone.status = InboundInvoicePaymentStatus::Claiming;

        Ok(clone)
    }

    pub(crate) fn check_payment_claimed(
        &self,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
        amount: Amount,
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
            Expired => bail!("Payment already expired"),
        }

        if let Some(invoice_amount) = self.invoice_amount {
            if amount < invoice_amount {
                warn!("Requested {invoice_amount} but claimed {amount}");
                // TODO(max): In the future, we might want to bail! instead
            }
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.recvd_amount = Some(amount);
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Checks whether this payment's invoice has expired. If so, and if the
    /// state transition to `Expired` is valid, returns a clone with the state
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
            Completed | Expired => return None,
        }

        // Validation complete; invoice expired and Expired transition is valid

        let mut clone = self.clone();
        clone.status = Expired;
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
    /// The amount received in this payment.
    pub amount: Amount,
    /// The amount we paid in on-chain fees (possibly arising from receiving
    /// our payment over a JIT channel) to receive this transaction.
    // TODO(max): Implement
    pub onchain_fees: Option<Amount>,
    /// The current status of the payment.
    pub status: InboundSpontaneousPaymentStatus,
    /// An optional personal note for this payment. Since there is no way for
    /// users to add the note at the time of receiving an inbound spontaneous
    /// payment, this field can only be added or updated later.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub note: Option<String>,
    /// When we first learned of this payment via [`PaymentClaimable`].
    pub created_at: TimestampMs,
    /// When this payment reached the `Completed` state.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
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
        amount: Amount,
    ) -> Self {
        Self {
            hash,
            preimage,
            amount,
            // TODO(max): Implement
            onchain_fees: None,
            status: InboundSpontaneousPaymentStatus::Claiming,
            note: None,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }

    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amount: Amount,
    ) -> anyhow::Result<Self> {
        use InboundSpontaneousPaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(amount == self.amount, "Amounts don't match");
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
        amount: Amount,
    ) -> anyhow::Result<Self> {
        use InboundSpontaneousPaymentStatus::*;

        ensure!(hash == self.hash, "Hashes don't match");
        ensure!(preimage == self.preimage, "Preimages don't match");
        ensure!(amount == self.amount, "Amounts don't match");

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

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip::json_unit_enum_backwards_compat;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser =
            r#"["InvoiceGenerated","Claiming","Completed","Expired"]"#;
        json_unit_enum_backwards_compat::<InboundInvoicePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["Claiming","Completed"]"#;
        json_unit_enum_backwards_compat::<InboundSpontaneousPaymentStatus>(
            expected_ser,
        );
    }
}
