use std::{collections::HashSet, num::NonZeroU64, sync::Arc};

use anyhow::{Context, anyhow, ensure};
use common::{ln::amount::Amount, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        LnClaimId, LxOfferId, LxPaymentHash, LxPaymentId, LxPaymentPreimage,
        LxPaymentSecret, PaymentKind,
    },
};
use lightning::events::PaymentPurpose;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::Event::{PaymentClaimable, PaymentClaimed},
    ln::channelmanager::ChannelManager,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(doc)]
use crate::command::create_invoice;
use crate::payments::{
    PaymentMetadata, PaymentV2, PaymentWithMetadata, manager::CheckedPayment,
};

// --- Helpers to delegate to the inner type --- //

// TODO(max): Switch this impl to `PaymentV2` once we switch payman to v2 only.
/// Helper to handle the [`PaymentV2`] and [`LnClaimCtx`] matching.
// Normally we don't want this much indirection, but the calling code is already
// doing lots of ugly matching (at a higher abstraction level), so in this case
// the separation makes both functions cleaner and easier to read.
impl PaymentWithMetadata {
    /// ## Precondition
    /// - The payment must not be finalized (`Completed` or `Expired`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn check_payment_claimable(
        &self,
        claim_ctx: LnClaimCtx,
        amount: Amount,
        skimmed_fee: Option<Amount>,
        now: TimestampMs,
    ) -> Result<CheckedPayment, ClaimableError> {
        if claim_ctx.kind() != self.payment.kind() {
            let claimkind = claim_ctx.kind();
            let paykind = self.payment.kind();
            return Err(ClaimableError::Replay(anyhow!(
                "Claim kind doesn't match stored payment kind: \
                 {claimkind} != {paykind}"
            )));
        }

        match (&self.payment, claim_ctx) {
            (
                PaymentV2::InboundInvoice(iip),
                LnClaimCtx::Bolt11Invoice {
                    preimage,
                    hash,
                    secret,
                    claim_id,
                },
            ) => {
                let checked_iip = iip.check_payment_claimable(
                    hash,
                    secret,
                    preimage,
                    claim_id,
                    amount,
                    skimmed_fee,
                    now,
                )?;
                let iipwm = PaymentWithMetadata {
                    payment: checked_iip,
                    metadata: self.metadata.clone(),
                };
                Ok(CheckedPayment(iipwm.into_enum()))
            }
            (
                PaymentV2::InboundOfferReusable(iorp),
                LnClaimCtx::Bolt12Offer(ctx),
            ) => Err(iorp.check_payment_claimable(ctx, amount)),
            // TODO(max): Implement for BOLT 12 refunds
            // (
            //     PaymentV2::Bolt12Refund(b12r),
            //     LnClaimCtx::Bolt12Refund {
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
                PaymentV2::InboundSpontaneous(isp),
                LnClaimCtx::Spontaneous {
                    preimage,
                    hash,
                    claim_id: _claim_id,
                },
            ) => Err(isp.check_payment_claimable(hash, preimage, amount)),
            _ => Err(ClaimableError::Replay(anyhow!(
                "Not an inbound LN payment, or purpose didn't match"
            ))),
        }
    }
}

// --- ClaimableError --- //

/// Errors that can happen while handling a [`PaymentClaimable`] event.
#[derive(Debug)]
pub enum ClaimableError {
    /// A correctness error that should cause the payment to retry so we can
    /// investigate.
    Replay(anyhow::Error),
    /// We may have persisted after [`PaymentClaimable`] but crashed before
    /// the `channel_manager.claim_funds`. When the event replays, we can
    /// ignore re-persist but still attempt to reclaim.
    IgnoreAndReclaim,
    /// Fail the HTLCs back and tell them it's their fault.
    FailBackHtlcsTheirFault,
    /// Persist failed.
    Persist(anyhow::Error),
}

impl ClaimableError {
    #[cfg(test)]
    pub(crate) fn is_replay(&self) -> bool {
        matches!(self, Self::Replay(_))
    }
}

// --- LnClaimCtx --- //

/// Common data used to handle a [`PaymentClaimable`]/[`PaymentClaimed`] event.
#[derive(Clone)]
pub enum LnClaimCtx {
    Bolt11Invoice {
        preimage: LxPaymentPreimage,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        // TODO(phlip9): make non-Option once we don't have replaying Claimed
        claim_id: Option<LnClaimId>,
    },
    Bolt12Offer(OfferClaimCtx),
    // // TODO(phlip9): BOLT12 refund
    // Bolt12Refund {
    //     preimage: LxPaymentPreimage,
    //     hash: LxPaymentHash,
    //     secret: LxPaymentSecret,
    //     claim_id: Option<LnClaimId>,
    //     context: Bolt12RefundContext,
    // },
    Spontaneous {
        preimage: LxPaymentPreimage,
        hash: LxPaymentHash,
        // TODO(phlip9): make non-Option once we don't have replaying Claimed
        claim_id: Option<LnClaimId>,
    },
}

/// Data used to handle a [`PaymentClaimable`]/[`PaymentClaimed`] event for an
/// [`InboundOfferReusablePaymentV2`].
#[derive(Clone)]
pub struct OfferClaimCtx {
    pub preimage: LxPaymentPreimage,
    // We don't have any BOLT12 offers pending, so we can assume claim id
    // is present.
    pub claim_id: LnClaimId,
    pub offer_id: LxOfferId,
    pub offer: Option<Arc<LxOffer>>,
    pub quantity: Option<NonZeroU64>,
    pub payer_note: Option<String>,
    // TODO(phlip9): use newtype
    pub payer_name: Option<String>,
}

impl LnClaimCtx {
    pub fn new(
        purpose: PaymentPurpose,
        hash: LxPaymentHash,
        claim_id: Option<LnClaimId>,
        offer: Option<LxOffer>,
    ) -> anyhow::Result<Self> {
        let no_preimage_msg = "We should always let LDK handle payment preimages for us by \
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
                Ok(Self::Bolt11Invoice {
                    preimage,
                    hash,
                    secret,
                    claim_id,
                })
            }
            PaymentPurpose::Bolt12OfferPayment {
                payment_preimage: _,
                payment_secret: _,
                payment_context: context,
            } => {
                debug_assert!(claim_id.is_some());
                let claim_id = claim_id
                    .context("BOLT12 offer payment must have a claim id")?;
                let offer_id = LxOfferId::from(context.offer_id);
                let quantity =
                    context.invoice_request.quantity.and_then(NonZeroU64::new);
                let payer_note =
                    context.invoice_request.payer_note_truncated.map(|s| s.0);
                // TODO(phlip9): use newtype
                let payer_name = context
                    .invoice_request
                    .human_readable_name
                    .map(|hrn| format!("{}@{}", hrn.user(), hrn.domain()));
                Ok(Self::Bolt12Offer(OfferClaimCtx {
                    preimage,
                    claim_id,
                    offer_id,
                    offer: offer.map(Arc::new),
                    quantity,
                    payer_note,
                    payer_name,
                }))
            }
            // TODO(phlip9): BOLT12 refunds
            PaymentPurpose::Bolt12RefundPayment { .. } => {
                debug_assert!(false, "TODO: BOLT12 refunds");
                Err(anyhow!("We don't support BOLT12 refunds yet"))
            }
            PaymentPurpose::SpontaneousPayment(_payment_preimage) =>
                Ok(Self::Spontaneous {
                    preimage,
                    hash,
                    claim_id,
                }),
        }
    }

    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::Bolt11Invoice { hash, .. } => LxPaymentId::Lightning(*hash),
            // TODO(phlip9): how to disambiguate single-use BOLT12 offer
            Self::Bolt12Offer(OfferClaimCtx { claim_id, .. }) =>
                LxPaymentId::OfferRecvReusable(*claim_id),
            Self::Spontaneous { hash, .. } => LxPaymentId::Lightning(*hash),
        }
    }

    pub fn preimage(&self) -> LxPaymentPreimage {
        match self {
            Self::Bolt11Invoice { preimage, .. } => *preimage,
            Self::Bolt12Offer(OfferClaimCtx { preimage, .. }) => *preimage,
            Self::Spontaneous { preimage, .. } => *preimage,
        }
    }

    /// Get the [`PaymentKind`] which corresponds to this [`LnClaimCtx`].
    pub fn kind(&self) -> PaymentKind {
        match self {
            Self::Bolt11Invoice { .. } => PaymentKind::Invoice,
            Self::Bolt12Offer(_) => PaymentKind::Offer,
            Self::Spontaneous { .. } => PaymentKind::Spontaneous,
        }
    }
}

// --- Inbound invoice payments --- //

/// A 'conventional' inbound payment which is facilitated by an invoice.
/// This struct is created when we call [`create_invoice`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundInvoicePaymentV2 {
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
    /// Contained in:
    /// - the [`PaymentClaimable`] and [`PaymentClaimed`] events.
    ///
    /// This id lets us disambiguate between (1) an event replay for this
    /// invoice (ok), and (2) a payer paying the same invoice multiple times
    /// (not ok), which should be fail the HTLCs back.
    ///
    /// It is the hash of the HTLC(s) paying a payment hash.
    //
    // Added in node-v0.7.4
    // - Older finalized payments will not have this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<LnClaimId>,

    /// The amount encoded in our invoice, if there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice_amount: Option<Amount>,
    /// The amount that we actually received. May be greater than the invoice
    /// amount. Populated iff we received a [`PaymentClaimable`] event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recvd_amount: Option<Amount>,
    /// The amount that was skimmed off of this payment as an extra fee taken
    /// by our channel counterparty. Populated during [`PaymentClaimable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skimmed_fee: Option<Amount>,
    /* TODO(max): Implement JIT channel fees
    /// The portion of the skimmed amount that was used to cover the on-chain
    /// fees incurred by a JIT channel opened to receive this payment.
    /// None if no on-chain fees were incurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_fee: Option<Amount>,
    */
    /// The current status of the payment.
    pub status: InboundInvoicePaymentStatus,

    /// When we created the invoice for this payment.
    /// Set to `Some(...)` on first persist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<TimestampMs>,
    /// When the invoice expires. Computed from the invoice's timestamp +
    /// expiry duration. `None` if the expiry timestamp overflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<TimestampMs>,
    /// When this payment either `Completed` or `Expired`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::VariantArray, Hash))]
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

impl InboundInvoicePaymentV2 {
    // Event sources:
    // - `create_invoice` API
    pub fn new(
        invoice: LxInvoice,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
    ) -> PaymentWithMetadata<Self> {
        let invoice_amount =
            invoice.0.amount_milli_satoshis().map(Amount::from_msat);
        let expires_at = invoice.expires_at().ok();
        let iip = Self {
            hash,
            secret,
            preimage,
            claim_id: None,
            invoice_amount,
            recvd_amount: None,
            skimmed_fee: None,
            // channel_fee: None,
            status: InboundInvoicePaymentStatus::InvoiceGenerated,
            created_at: None,
            expires_at,
            finalized_at: None,
        };

        let metadata = PaymentMetadata {
            id: iip.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: Some(Arc::new(invoice)),
            offer: None,
            note: None,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
        };

        PaymentWithMetadata {
            payment: iip,
            metadata,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed` or `Expired`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        secret: LxPaymentSecret,
        preimage: LxPaymentPreimage,
        // TODO(phlip9): make non-Option once all replaying Claimable events
        // drain in prod.
        claim_id: Option<LnClaimId>,
        amount: Amount,
        skimmed_fee: Option<Amount>,
        now: TimestampMs,
    ) -> Result<Self, ClaimableError> {
        use InboundInvoicePaymentStatus::*;

        // Payment state machine errors
        if hash != self.hash {
            return Err(ClaimableError::Replay(anyhow::anyhow!(
                "Hashes don't match"
            )));
        }
        if preimage != self.preimage {
            return Err(ClaimableError::Replay(anyhow::anyhow!(
                "Preimages don't match"
            )));
        }
        if secret != self.secret {
            return Err(ClaimableError::Replay(anyhow::anyhow!(
                "Secrets don't match"
            )));
        }

        // The PaymentClaimable docs have a note that LDK will not stop an
        // inbound payment from being paid multiple times. We should fail the
        // payment in this case because:
        // - This messes up (or significantly complicates) our accounting
        // - This likely reflects an error on the receiver's part (reusing the
        //   same invoice for multiple payments, which would allow any nodes
        //   along the first payment path to steal subsequent payments)
        // - We should not allow payments to go through, in order to teach users
        //   that this is not an acceptable way to use lightning, because it is
        //   not safe. It is not hard to imagine users developing the
        //   misconception that it is safe to reuse invoices if duplicate
        //   payments actually do succeed.

        // Fail the HTLCs back if the payer is trying to pay the same invoice
        // twice, i.e., the same payment hash is paid with a different LnClaimId
        if let Some(claim_id) = claim_id
            && let Some(this_claim_id) = self.claim_id
            && this_claim_id != claim_id
        {
            warn!("payer is trying to pay the same payment hash twice");
            return Err(ClaimableError::FailBackHtlcsTheirFault);
        }

        match self.status {
            InvoiceGenerated => (),
            // [Idempotency]
            // We may have persisted after `PaymentClaimable` but crashed before
            // the `channel_manager.claim_funds`. In the event replay, we can
            // ignore re-persist but still attempt to reclaim.
            Claiming => {
                warn!("claimable on invoice payment that's already claiming");
                return Err(ClaimableError::IgnoreAndReclaim);
            }
            Completed | Expired => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status
            ),
        }

        // BOLT11: "A payee: after the timestamp plus expiry has passed: SHOULD
        // NOT accept a payment."
        let is_expired = self
            .expires_at
            .map(|expires_at| expires_at <= now)
            .unwrap_or(false);
        if is_expired {
            // Ignore and let the invoice expiry checker handle this.
            warn!("claimable on invoice payment after it expired");
            return Err(ClaimableError::FailBackHtlcsTheirFault);
        }

        // TODO(phlip9): charge the user for LSP fees on inbound as a skimmed
        // amount, but only up to the expected fee rate and no more.
        if let Some(invoice_amount) = self.invoice_amount
            && amount < invoice_amount
        {
            warn!("Requested {invoice_amount} but claiming {amount}");
        }

        // TODO(max): In the future, check for on-chain fees here

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = InboundInvoicePaymentStatus::Claiming;
        clone.claim_id = claim_id;
        clone.recvd_amount = Some(amount);
        clone.skimmed_fee = skimmed_fee;

        Ok(clone)
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed` or `Expired`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
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
            Completed | Expired => {
                unreachable!(
                    "caller ensures payment is not already finalized. \
                     {id} is already {status:?}",
                    id = self.id(),
                    status = self.status
                );
            }
        }

        // TODO(phlip9): don't accept underpaying payments
        if let Some(invoice_amount) = self.invoice_amount
            && amount < invoice_amount
        {
            warn!("Requested {invoice_amount} but claimed {amount}");
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
    /// ## Precondition
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `PaymentsManager::spawn_payment_expiry_checker` task
    pub(crate) fn check_invoice_expiry(
        &self,
        now: TimestampMs,
    ) -> Option<Self> {
        use InboundInvoicePaymentStatus::*;

        // If not expired yet, do nothing.
        let is_expired = self
            .expires_at
            .map(|expires_at| expires_at < now)
            .unwrap_or(false);
        if !is_expired {
            return None;
        }

        match self.status {
            InvoiceGenerated => (),
            // We are already claiming the payment; too late to time it out now.
            Claiming => return None,
            Completed | Expired => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status,
            ),
        }

        // Validation complete; invoice expired and Expired transition is valid

        let mut clone = self.clone();
        clone.status = Expired;
        clone.finalized_at = Some(now);

        Some(clone)
    }
}

// --- Inbound BOLT12 offer payments --- //

// TODO(phlip9): single-use BOLT12 offer payments

/// An inbound, _reusable_ BOLT12 offer payment. This struct is created when we
/// get a [`PaymentClaimable`] event, with
/// [`PaymentPurpose::Bolt12OfferPayment`].
//
// TODO(phlip9): we'll need to maintain a separate `Offer` metadata store to
// correlate `offer_id` with the actual offer. This is mostly useful to get our
// original offer `description`. This would need to be optional though to
// support externally generated offers (e.g. dumb shopify plugin generates an
// offer without letting the node know).
//
// Added in `node-v0.7.8`
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundOfferReusablePaymentV2 {
    /// The claim id uniquely identifies a single payment for this offer.
    /// It is the hash of the HTLC(s) paying a payment hash.
    pub claim_id: LnClaimId,
    /// Unique identifier for the original offer, which may be paid multiple
    /// times.
    pub offer_id: LxOfferId,
    /// The payment preimage for this offer payment.
    pub preimage: LxPaymentPreimage,

    /// The amount we received for this payment.
    pub amount: Amount,

    /// The amount that was skimmed off of this payment as an extra fee taken
    /// by our channel counterparty. Populated during [`PaymentClaimable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skimmed_fee: Option<Amount>,
    /* TODO(max): Implement JIT channel fees
    /// The portion of the skimmed amount that was used to cover the on-chain
    /// fees incurred by a JIT channel opened to receive this payment.
    /// None if no on-chain fees were incurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_fee: Option<Amount>,
    */
    /// The current payment status.
    pub status: InboundOfferReusablePaymentStatus,
    /// When we first learned of this payment via [`PaymentClaimable`].
    /// Set to `Some(...)` on first persist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<TimestampMs>,
    /// When this payment reached the `Completed` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::VariantArray, Hash))]
pub enum InboundOfferReusablePaymentStatus {
    /// We received a [`PaymentClaimable`] event.
    Claiming,
    /// We received a [`PaymentClaimed`] event.
    Completed,
    // NOTE: We don't have a "Failed" case here because (as Matt says) if you
    // call ChannelManager::claim_funds we should always get the
    // PaymentClaimed event back. If for some reason this turns out not to
    // be true (i.e. we observe a number of inbound reusable offer payments
    // stuck in the "claiming" state), then we can add a "Failed" state
    // here. https://discord.com/channels/915026692102316113/978829624635195422/1085427776070365214
}

impl InboundOfferReusablePaymentV2 {
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn new(
        ctx: OfferClaimCtx,
        amount: Amount,
        skimmed_fee: Option<Amount>,
    ) -> PaymentWithMetadata<Self> {
        let iorp = Self {
            claim_id: ctx.claim_id,
            offer_id: ctx.offer_id,
            preimage: ctx.preimage,
            amount,
            skimmed_fee,
            // channel_fee: None,
            status: InboundOfferReusablePaymentStatus::Claiming,
            created_at: None,
            finalized_at: None,
        };

        let metadata = PaymentMetadata {
            id: iorp.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: None,
            offer: ctx.offer,
            note: None,
            payer_name: ctx.payer_name,
            payer_note: ctx.payer_note,
            priority: None,
            quantity: ctx.quantity,
            replacement_txid: None,
        };
        PaymentWithMetadata {
            payment: iorp,
            metadata,
        }
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    //
    // We're likely replaying a `PaymentClaimable` event that we partially
    // handled before crashing.
    pub(crate) fn check_payment_claimable(
        &self,
        ctx: OfferClaimCtx,
        amount: Amount,
    ) -> ClaimableError {
        use InboundOfferReusablePaymentStatus::*;

        // Catch payment state machine errors
        if ctx.preimage != self.preimage {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Preimages don't match"
            ));
        }
        if ctx.offer_id != self.offer_id {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Offer ids don't match"
            ));
        }
        if ctx.claim_id != self.claim_id {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Claim ids don't match"
            ));
        }
        if amount != self.amount {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Amounts don't match"
            ));
        }

        match self.status {
            Claiming => (),
            Completed => {
                unreachable!(
                    "caller ensures payment is not already finalized. \
                     {id} is already {status:?}",
                    id = self.id(),
                    status = self.status
                );
            }
        }

        // There is no state to update, but this may be a replay after crash,
        // so try to reclaim
        ClaimableError::IgnoreAndReclaim
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed` or `Expired`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
    pub(crate) fn check_payment_claimed(
        &self,
        ctx: OfferClaimCtx,
        amount: Amount,
    ) -> anyhow::Result<Self> {
        use InboundOfferReusablePaymentStatus::*;

        ensure!(ctx.preimage == self.preimage, "Preimages don't match");
        ensure!(ctx.claim_id == self.claim_id, "Claim ids don't match");
        ensure!(ctx.offer_id == self.offer_id, "Offer ids don't match");
        ensure!(amount == self.amount, "Amounts don't match");

        match self.status {
            Claiming => (),
            Completed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status
            ),
        }

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OfferRecvReusable(self.claim_id)
    }
}

// --- Inbound spontaneous payments --- //

/// An inbound spontaneous (keysend) payment. This struct is created when we
/// get a [`PaymentClaimable`] event, with
/// [`PaymentPurpose::SpontaneousPayment`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundSpontaneousPaymentV2 {
    /// Given by [`PaymentClaimable`] and [`PaymentClaimed`].
    pub hash: LxPaymentHash,
    /// Given by [`PaymentPurpose`].
    pub preimage: LxPaymentPreimage,

    /// The amount received in this payment.
    pub amount: Amount,

    /// The amount that was skimmed off of this payment as an extra fee taken
    /// by our channel counterparty. Populated during [`PaymentClaimable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skimmed_fee: Option<Amount>,
    /* TODO(max): Implement JIT channel fees
    /// The portion of the skimmed amount that was used to cover the on-chain
    /// fees incurred by a JIT channel opened to receive this payment.
    /// None if no on-chain fees were incurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_fee: Option<Amount>,
    */
    /// The current status of the payment.
    pub status: InboundSpontaneousPaymentStatus,

    /// When we first learned of this payment via [`PaymentClaimable`].
    /// Set to `Some(...)` on first persist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<TimestampMs>,
    /// When this payment reached the `Completed` state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::VariantArray, Hash))]
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

impl InboundSpontaneousPaymentV2 {
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn new(
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amount: Amount,
        skimmed_fee: Option<Amount>,
    ) -> PaymentWithMetadata<Self> {
        let isp = Self {
            hash,
            preimage,
            amount,
            skimmed_fee,
            // channel_fee: None,
            status: InboundSpontaneousPaymentStatus::Claiming,
            created_at: None,
            finalized_at: None,
        };

        let metadata = PaymentMetadata::empty(isp.id());

        PaymentWithMetadata {
            payment: isp,
            metadata,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        amount: Amount,
    ) -> ClaimableError {
        use InboundSpontaneousPaymentStatus::*;

        // Payment state machine errors
        if hash != self.hash {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Hashes don't match"
            ));
        }
        if preimage != self.preimage {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Preimages don't match"
            ));
        }
        if amount != self.amount {
            return ClaimableError::Replay(anyhow::anyhow!(
                "Amounts don't match"
            ));
        }

        match self.status {
            Claiming => (),
            Completed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status
            ),
        }

        // There is no state to update, but this may be a replay after crash,
        // so try to reclaim
        ClaimableError::IgnoreAndReclaim
    }

    /// ## Precondition
    /// - The payment must not be finalized (`Completed`).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
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
            Completed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status
            ),
        }

        // Everything ok; return a clone with the updated state
        let mut clone = self.clone();
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use common::test_utils::arbitrary;
    use lexe_api::types::{
        invoice::arbitrary_impl::LxInvoiceParams, payments::LxPaymentPreimage,
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        prelude::Just,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };
    use strum::VariantArray;

    use super::*;

    impl Arbitrary for InboundInvoicePaymentV2 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let preimage = any::<LxPaymentPreimage>();
            let preimage_invoice = preimage.prop_ind_flat_map2(|preimage| {
                any_with::<LxInvoice>(LxInvoiceParams {
                    payment_preimage: Some(preimage),
                })
            });

            let claim_id = any::<LnClaimId>();
            let recvd_amount = any::<Amount>();
            let skimmed_fee = any::<Amount>();
            let status = any_with::<InboundInvoicePaymentStatus>(pending_only);
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_iip = move |(
                preimage_invoice,
                claim_id,
                recvd_amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )| {
                use InboundInvoicePaymentStatus::*;

                let (preimage, invoice): (LxPaymentPreimage, LxInvoice) =
                    preimage_invoice;
                let hash = invoice.payment_hash();
                let secret = invoice.payment_secret();
                let invoice_amount = invoice.amount();
                let expires_at = invoice.expires_at().ok();
                let claim_id = match status {
                    InvoiceGenerated | Expired => None,
                    Claiming | Completed => Some(claim_id),
                };
                let recvd_amount = match status {
                    InvoiceGenerated | Expired => None,
                    Claiming | Completed => Some(recvd_amount),
                };
                let skimmed_fee = match status {
                    InvoiceGenerated | Expired => None,
                    Claiming | Completed => Some(skimmed_fee),
                };

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed | Expired)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed | Expired))
                };

                InboundInvoicePaymentV2 {
                    hash,
                    secret,
                    preimage,
                    claim_id,
                    invoice_amount,
                    recvd_amount,
                    skimmed_fee,
                    // channel_fee: None,
                    status,
                    created_at,
                    expires_at,
                    finalized_at,
                }
            };

            (
                preimage_invoice,
                claim_id,
                recvd_amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )
                .prop_map(gen_iip)
                .boxed()
        }
    }

    impl Arbitrary for InboundInvoicePaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            if pending_only {
                prop_oneof![
                    Just(InboundInvoicePaymentStatus::InvoiceGenerated),
                    Just(InboundInvoicePaymentStatus::Claiming),
                ]
                .boxed()
            } else {
                proptest::sample::select(InboundInvoicePaymentStatus::VARIANTS)
                    .boxed()
            }
        }
    }

    impl Arbitrary for InboundOfferReusablePaymentV2 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let preimage = any::<LxPaymentPreimage>();
            let claim_id = any::<LnClaimId>();
            let offer_id = any::<LxOfferId>();
            let amount = any::<Amount>();
            let skimmed_fee = any::<Amount>();
            let status =
                any_with::<InboundOfferReusablePaymentStatus>(pending_only);
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_iorp = move |(
                preimage,
                claim_id,
                offer_id,
                amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )| {
                use InboundOfferReusablePaymentStatus::*;

                let skimmed_fee = Some(skimmed_fee);

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed))
                };

                InboundOfferReusablePaymentV2 {
                    claim_id,
                    offer_id,
                    preimage,
                    amount,
                    skimmed_fee,
                    // channel_fee: None,
                    status,
                    created_at,
                    finalized_at,
                }
            };

            (
                preimage,
                claim_id,
                offer_id,
                amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )
                .prop_map(gen_iorp)
                .boxed()
        }
    }

    impl Arbitrary for InboundOfferReusablePaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            if pending_only {
                Just(InboundOfferReusablePaymentStatus::Claiming).boxed()
            } else {
                proptest::sample::select(
                    InboundOfferReusablePaymentStatus::VARIANTS,
                )
                .boxed()
            }
        }
    }

    impl Arbitrary for InboundSpontaneousPaymentV2 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let hash = any::<LxPaymentHash>();
            let preimage = any::<LxPaymentPreimage>();
            let amount = any::<Amount>();
            let skimmed_fee = any::<Amount>();
            let status =
                any_with::<InboundSpontaneousPaymentStatus>(pending_only);
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_isp = move |(
                hash,
                preimage,
                amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )| {
                use InboundSpontaneousPaymentStatus::*;

                let skimmed_fee = Some(skimmed_fee);

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed))
                };

                InboundSpontaneousPaymentV2 {
                    hash,
                    preimage,
                    amount,
                    skimmed_fee,
                    // channel_fee: None,
                    status,
                    created_at,
                    finalized_at,
                }
            };

            (
                hash,
                preimage,
                amount,
                skimmed_fee,
                status,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )
                .prop_map(gen_isp)
                .boxed()
        }
    }

    impl Arbitrary for InboundSpontaneousPaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            if pending_only {
                Just(InboundSpontaneousPaymentStatus::Claiming).boxed()
            } else {
                proptest::sample::select(
                    InboundSpontaneousPaymentStatus::VARIANTS,
                )
                .boxed()
            }
        }
    }
}

#[cfg(test)]
mod test {
    use common::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::arbitrary::any;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser =
            r#"["invoice_generated","claiming","completed","expired"]"#;
        roundtrip::json_unit_enum_backwards_compat::<InboundInvoicePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["claiming","completed"]"#;
        roundtrip::json_unit_enum_backwards_compat::<
            InboundOfferReusablePaymentStatus,
        >(expected_ser);

        let expected_ser = r#"["claiming","completed"]"#;
        roundtrip::json_unit_enum_backwards_compat::<
            InboundSpontaneousPaymentStatus,
        >(expected_ser);
    }

    #[ignore]
    #[test]
    fn inbound_invoice_sample_data() {
        use std::collections::HashMap;

        let mut rng = FastRng::from_u64(202503311959);
        let values = arbitrary::gen_values(
            &mut rng,
            any::<InboundInvoicePaymentV2>(),
            100,
        );

        // Just give me one per status
        let values = values
            .into_iter()
            .map(|iip| (iip.status, iip))
            .collect::<HashMap<_, InboundInvoicePaymentV2>>();

        for iip in values.values() {
            println!("{}", serde_json::to_string(&iip).unwrap());
        }
    }
}
