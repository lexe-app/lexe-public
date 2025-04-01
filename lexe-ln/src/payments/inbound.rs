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
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    // TODO(phlip9): idempotency audit
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
    pub note: Option<String>,
    /// When we created the invoice for this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Expired`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray, Hash))]
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
    // Event sources:
    // - `create_invoice` API
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

    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    // TODO(phlip9): idempotency audit
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

    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
    // TODO(phlip9): idempotency audit
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
    //
    // Event sources:
    // - `PaymentsManager::spawn_invoice_expiry_checker` task
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
#[serde(rename_all = "snake_case")]
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
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
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

    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    // TODO(phlip9): idempotency audit
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

    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
    // TODO(phlip9): idempotency audit
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
mod arb {
    use arbitrary::{any_duration, any_option_simple_string};
    use common::{
        ln::{
            invoice::arbitrary_impl::LxInvoiceParams,
            payments::{LxPaymentPreimage, PaymentStatus},
        },
        sat,
    };
    use proptest::{
        arbitrary::{any, any_with, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for InboundInvoicePayment {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let preimage = any::<LxPaymentPreimage>();
            let preimage_invoice = preimage.prop_ind_flat_map2(|preimage| {
                any_with::<LxInvoice>(LxInvoiceParams {
                    payment_preimage: Some(preimage),
                })
            });

            let recvd_amount = any::<Option<Amount>>();
            let status = any::<InboundInvoicePaymentStatus>();
            let note = any_option_simple_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = any_duration();

            let gen_iip = |(
                preimage_invoice,
                recvd_amount,
                status,
                note,
                created_at,
                finalized_after,
            )| {
                use InboundInvoicePaymentStatus::*;
                let (preimage, invoice): (LxPaymentPreimage, LxInvoice) =
                    preimage_invoice;
                let hash = invoice.payment_hash();
                let secret = invoice.payment_secret();
                let invoice_amount = invoice.amount();
                let recvd_amount: Option<Amount> = recvd_amount;
                let recvd_amount = match status {
                    InvoiceGenerated => None,
                    Claiming | Completed => Some(
                        recvd_amount
                            .or(invoice_amount)
                            // handle amount-less invoice
                            .unwrap_or(sat!(1_234)),
                    ),
                    Expired => recvd_amount,
                };
                InboundInvoicePayment {
                    invoice: Box::new(invoice),
                    hash,
                    secret,
                    preimage,
                    invoice_amount,
                    recvd_amount,
                    // TODO(phlip9): it looks like we don't implement this yet
                    onchain_fees: None,
                    status,
                    note,
                    created_at,
                    finalized_at: PaymentStatus::from(status)
                        .is_finalized()
                        .then_some(created_at.saturating_add(finalized_after)),
                }
            };

            (
                preimage_invoice,
                recvd_amount,
                status,
                note,
                created_at,
                finalized_after,
            )
                .prop_map(gen_iip)
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use arbitrary::gen_values;
    use common::{
        rng::FastRng,
        test_utils::{roundtrip::json_unit_enum_backwards_compat, snapshot},
    };
    use proptest::arbitrary::any;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser =
            r#"["invoice_generated","claiming","completed","expired"]"#;
        json_unit_enum_backwards_compat::<InboundInvoicePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["claiming","completed"]"#;
        json_unit_enum_backwards_compat::<InboundSpontaneousPaymentStatus>(
            expected_ser,
        );
    }

    #[test]
    fn inbound_invoice_deser_compat() {
        let inputs = r#"
--- InvoiceGenerated
{"InboundInvoice":{"invoice":"lnbc7363509714019145550p1fh8xlrthp5y0ud564d7780074s2pllju2ap7jns0pfqtta9aku6t8mhwvm0j8qpp576k5h3sgt39apacz2ur7k9p50ghhjnvahmtkwuejp2309nrpwsgssp5p3wwm4cmm3a3j6uwaayhuawqf4lf290md8wjmdhpse8p9785sa3q9qyysgqcqrkyf3yxm6n8qklnqh4e3vud7wnx3rp3759up5kc3dulnvz84a0ws25qp0kh8ayymarg8qjn2cawytgztul68vf8s6zscu4x5jfpu03du2qsql3n0ea","hash":"f6ad4bc6085c4bd0f7025707eb14347a2f794d9dbed76773320aa2f2cc617411","secret":"0c5cedd71bdc7b196b8eef497e75c04d7e9515fb69dd2db6e1864e12f8f48762","preimage":"3338296898b6b57ff4bd3526977fd6bc433e5678779334bc4720239fa34214d4","invoice_amount":"736350971401914.555","recvd_amount":null,"onchain_fees":null,"status":"invoice_generated","note":null,"created_at":2395485827019270500,"finalized_at":null}}
--- Claiming
{"InboundInvoice":{"invoice":"lnbcrt14315814875280385750p1jjk2hgxhp59f9glq0mx9xw3yvec466jjrd445rxgefj4h72agwfglale6fkl3spp5q8g0z3rpe2f6tgenkc8e0yymfdvdppt9fvsjj4kweytpz49lvzwssp5e3jdlpgvqm5jy8ffarzp4a8qgqzyrantvefdn0p9navpn88dz82q9q2sqqqqqysgqcqrr48r9yqfteetvkhuv5pc2un4605raj7zdvn055vkkpmpsvl24s04mzcvleg2m6z6mjqw93ndyaw3ufq8j8t2hkkgp60tuhjmh54h2mxygrcyquapr40gs43rad3k3thjnymarjndcl3hhwrvp4dk3vg5027gw4s6z28t596upqmrvl3n0k6hu97p97lhsu73k4jt7yn6wqmcjjxaagggeg8h2s0jxvkgx3qfgzztzuvcsn6y7je33awak9kdsmtlj29aqf25rggt582cc8tx83r0qlnwef6etnzfws9zffd7553yxl2azn5k89fwqj4a4f7tgn5xflm2q60d6zth32h5v9yaa8qyzezgmsfkn7g44gzmn9km7scd0jgm4etgpkhcq2u","hash":"01d0f14461ca93a5a333b60f97909b4b58d085654b212956cec9161154bf609d","secret":"cc64df850c06e9221d29e8c41af4e0400441f66b6652d9bc259f58199ced11d4","preimage":"780a5c91bb7dc7e6dc531cc6fc5560108e00a41b26cb4c5635fffea620589cf6","invoice_amount":"1431581487528038.575","recvd_amount":"631803834701528.778","onchain_fees":null,"status":"claiming","note":null,"created_at":1543439437847952694,"finalized_at":null}}
--- Expired
{"InboundInvoice":{"invoice":"lntb14e0n6q4dz9xumhjup3x9j9xw26wpq5563s2dzkcdm0wuck5v35wfk8qj6kxsurz3m9xfg8yj2rwserzpp5yzwmvkcq55hdfrvjhptswwzgpw0lx9jj7s6pwpsp8pgsd885sspqsp5rzvqgh4e767pj5sw82qdy5a8hha92j8wmaa5khtjt2jype525qvs9qyysgqcqre32r9yq2y0jqz9nstk27c7khlytgt8tvffelnxmv3390uc9k9wl487p20sxew42s3m0hq8hpegg3tr5u53n5qdsypndt8h348355z546tprdkn94hlxdgrp9ggnsksqa7e96tl38k8rdggjxhykujewj6u2auydhc5r3dctfvsr4fmq4cj9hjqdgfykv4eqeujlgldu5tlkzwm3zg8gdm67kr6p8hhy63kwt85rxga2ktu4lkmzkf222udt44y37utqrkfe206wlyyu3sq285nms","hash":"209db65b00a52ed48d92b8570738480b9ff31652f4341706013851069cf48402","secret":"1898045eb9f6bc19520e3a80d253a7bdfa5548eedf7b4b5d725aa440e68aa019","preimage":"11ecd0c5af67c11fd03036c91b30a95db2ec97b2dd2ac4b8da39865215ed745a","invoice_amount":null,"recvd_amount":"1549527423313541.737","onchain_fees":null,"status":"expired","note":null,"created_at":5209058120350254120,"finalized_at":9223372036854775807}}
--- Completed
{"InboundInvoice":{"invoice":"lntbs575933122507938450p17zdsk3uhp5hkwcx7t29pmgr9a9c2qapr994ag7fn920mz7zvs98nu20wzcgv5spp5yuq7s8fl6j56vga6806en6zvwyq28xyfx75y0v5dtuf9rau3h4gqsp5l4r892flu83tmc5apkvyrcsz5ems5222wc6wyq5m35kmxgx32kss9qyysgqcqypu4jxq8lllllllr9yqgz2dq9lhhxfau7kq0gdvm2trf0kf8th9va2flzrxvcjrwfep0zvaajemp576zdx2jhdktt3cxravhqa5qpjmfdwf49g3vakzf64t0ulppsx5c058rmsmeprtjyq7h976r4grn2mpa03xcp42yw655h4cz9pcauwcrjs6pac02yjg0hy4dy7k6eekd5vpv423u70ypp738zc8m3ze7m56d255vn96n5dugkmww32adexzx9kvk9hy8s46ngx8f6dxc4mxvas5vgptckxsl","hash":"2701e81d3fd4a9a623ba3bf599e84c7100a3988937a847b28d5f1251f791bd50","secret":"fd4672a93fe1e2bde29d0d9841e202a6770a294a7634e2029b8d2db320d155a1","preimage":"1e444fb7d12ca78ef4028adc85fd0e50f4ad51a8c12df6362a68fad4e5f60d39","invoice_amount":"57593312250793.845","recvd_amount":"57593312250793.845","onchain_fees":null,"status":"completed","note":"ZTCC2PqaX1yiZNOhvyaF618obYh0c3lGX3G5aAMf0a87pw420f4O078RKAn53C2E1hMKc1b","created_at":7040449765819823150,"finalized_at":9223372036854775807}}
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let iip: Payment = serde_json::from_str(input).unwrap();
            let _ = serde_json::to_string(&iip).unwrap();
        }
    }

    #[ignore]
    #[test]
    fn inbound_invoice_sample_data() {
        let mut rng = FastRng::from_u64(202503311959);
        let values = gen_values(&mut rng, any::<InboundInvoicePayment>(), 100);

        // Just give me one per status
        let values = values
            .into_iter()
            .map(|iip| (iip.status, Payment::from(iip)))
            .collect::<HashMap<_, _>>();

        for iip in values.values() {
            println!("{}", serde_json::to_string(&iip).unwrap());
        }
    }
}
