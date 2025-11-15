use std::num::NonZeroU64;

use anyhow::{Context, ensure};
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{ln::amount::Amount, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    payments::{
        LnClaimId, LxOfferId, LxPaymentHash, LxPaymentId, LxPaymentPreimage,
        LxPaymentSecret,
    },
};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::{
        Event::{PaymentClaimable, PaymentClaimed},
        PaymentPurpose,
    },
    ln::channelmanager::ChannelManager,
};
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::command::create_invoice;
use crate::payments::{
    PaymentMetadata, PaymentWithMetadata,
    inbound::{
        ClaimableError, InboundInvoicePaymentStatus, InboundInvoicePaymentV2,
        InboundOfferReusablePaymentStatus, InboundSpontaneousPaymentStatus,
        OfferClaimCtx,
    },
};

// --- Inbound invoice payments --- //

/// A 'conventional' inbound payment which is facilitated by an invoice.
/// This struct is created when we call [`create_invoice`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundInvoicePaymentV1 {
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
    pub claim_id: Option<LnClaimId>,
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

impl InboundInvoicePaymentV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }
}

impl From<InboundInvoicePaymentV1>
    for PaymentWithMetadata<InboundInvoicePaymentV2>
{
    fn from(v1: InboundInvoicePaymentV1) -> Self {
        let payment = InboundInvoicePaymentV2 {
            hash: v1.hash,
            secret: v1.secret,
            preimage: v1.preimage,
            claim_id: v1.claim_id,
            invoice_amount: v1.invoice_amount,
            recvd_amount: v1.recvd_amount,
            sender_intended_amount: None,
            skimmed_fee: None,
            onchain_fee: v1.onchain_fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            expires_at: v1.invoice.expires_at().ok(),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            address: None,
            invoice: Some(*v1.invoice),
            offer: None,
            priority: None,
            replacement_txid: None,
            note: v1.note,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<InboundInvoicePaymentV2>>
    for InboundInvoicePaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<InboundInvoicePaymentV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let InboundInvoicePaymentV2 {
            hash,
            secret,
            preimage,
            claim_id,
            invoice_amount,
            recvd_amount,
            sender_intended_amount: _,
            skimmed_fee: _,
            onchain_fee: onchain_fees,
            status,
            created_at,
            expires_at: _expires_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            address: _,
            invoice,
            offer: _,
            priority: _,
            replacement_txid: _,
            note,
        } = pwm.metadata;

        Ok(Self {
            invoice: Box::new(invoice.context("Missing invoice")?),
            hash,
            secret,
            preimage,
            claim_id,
            invoice_amount,
            recvd_amount,
            onchain_fees,
            status,
            note,
            created_at: created_at.context("Missing created_at")?,
            finalized_at,
        })
    }
}

// --- Inbound BOLT12 offer payments --- //

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
pub struct InboundOfferReusablePaymentV1 {
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
    // TODO(phlip9): impl
    // /// The fees skimmed by the LSP for forwarding this payment.
    // pub lsp_fees: Amount,
    // /// The amount we paid for a JIT channel open.
    // pub onchain_fees: Option<Amount>,
    /// The number of items the payer bought.
    pub quantity: Option<NonZeroU64>,
    /// The current payment status.
    pub status: InboundOfferReusablePaymentStatus,
    /// An optional personal note for this payment.
    pub note: Option<String>,
    /// A payer-provided note for this payment. LDK truncates this to
    /// [`PAYER_NOTE_LIMIT`](lightning::offers::invoice_request::PAYER_NOTE_LIMIT)
    /// bytes (512 B as of 2025-04-22).
    pub payer_note: Option<String>,
    /// The payer's self-reported human-readable name.
    // TODO(phlip9): newtype
    pub payer_name: Option<String>,
    /// When we first learned of this payment via [`PaymentClaimable`].
    pub created_at: TimestampMs,
    /// When this payment reached the `Completed` state.
    pub finalized_at: Option<TimestampMs>,
}

impl InboundOfferReusablePaymentV1 {
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    pub(crate) fn new(
        ctx: OfferClaimCtx,
        amount: Amount,
        now: TimestampMs,
    ) -> Self {
        Self {
            claim_id: ctx.claim_id,
            offer_id: ctx.offer_id,
            preimage: ctx.preimage,
            amount,
            quantity: ctx.quantity,
            status: InboundOfferReusablePaymentStatus::Claiming,
            note: None,
            payer_note: ctx.payer_note,
            payer_name: ctx.payer_name,
            created_at: now,
            finalized_at: None,
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

    /// The total fees we paid to receive this payment
    #[inline]
    pub(crate) const fn fees(&self) -> Amount {
        // TODO(phlip9): impl LSP skimming to charge receiver for fees
        Amount::ZERO
    }
}

// --- Inbound spontaneous payments --- //

/// An inbound spontaneous (`keysend`) payment. This struct is created when we
/// get a [`PaymentClaimable`] event, with
/// [`PaymentPurpose::SpontaneousPayment`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundSpontaneousPaymentV1 {
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
    pub note: Option<String>,
    /// When we first learned of this payment via [`PaymentClaimable`].
    pub created_at: TimestampMs,
    /// When this payment reached the `Completed` state.
    pub finalized_at: Option<TimestampMs>,
}

impl InboundSpontaneousPaymentV1 {
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

    /// ## Precondition
    /// - The payment must not be finalized (`Completed` or `Expired`).
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
    /// - The payment must not be finalized (`Completed` or `Expired`).
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
    use lexe_api::types::{
        invoice::arbitrary_impl::LxInvoiceParams,
        offer::MaxQuantity,
        payments::{LxPaymentPreimage, PaymentStatus},
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for InboundInvoicePaymentV1 {
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
            let status = any_with::<InboundInvoicePaymentStatus>(pending_only);
            let note = any_option_simple_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = any_duration();

            let gen_iip = move |(
                preimage_invoice,
                claim_id,
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
                let claim_id = match status {
                    InvoiceGenerated | Expired => None,
                    Claiming | Completed => Some(claim_id),
                };
                let recvd_amount = match status {
                    InvoiceGenerated | Expired => None,
                    Claiming | Completed => Some(recvd_amount),
                };
                let created_at: TimestampMs = created_at; // provides type hint
                let finalized_at = if pending_only {
                    None
                } else {
                    let finalized_at =
                        created_at.saturating_add(finalized_after);
                    PaymentStatus::from(status)
                        .is_finalized()
                        .then_some(finalized_at)
                };

                InboundInvoicePaymentV1 {
                    invoice: Box::new(invoice),
                    hash,
                    secret,
                    preimage,
                    claim_id,
                    invoice_amount,
                    recvd_amount,
                    // TODO(phlip9): it looks like we don't implement this yet
                    onchain_fees: None,
                    status,
                    note,
                    created_at,
                    finalized_at,
                }
            };

            (
                preimage_invoice,
                claim_id,
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

    impl Arbitrary for InboundOfferReusablePaymentV1 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let preimage = any::<LxPaymentPreimage>();
            let claim_id = any::<LnClaimId>();
            let offer_id = any::<LxOfferId>();
            let amount = any::<Amount>();
            let quantity = any::<Option<MaxQuantity>>()
                .prop_map(|opt_q| opt_q.map(|q| q.0));
            let status =
                any_with::<InboundOfferReusablePaymentStatus>(pending_only);
            let note = any_option_simple_string();
            let payer_note = any_option_simple_string();
            // TODO(phlip9): use newtype
            let payer_name = any_option_simple_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = any_duration();

            let gen_iip = move |(
                preimage,
                claim_id,
                offer_id,
                amount,
                quantity,
                status,
                note,
                payer_note,
                payer_name,
                created_at,
                finalized_after,
            )| {
                let created_at: TimestampMs = created_at; // provides type hint
                let finalized_at = if pending_only {
                    None
                } else {
                    let finalized_at =
                        created_at.saturating_add(finalized_after);
                    PaymentStatus::from(status)
                        .is_finalized()
                        .then_some(finalized_at)
                };

                InboundOfferReusablePaymentV1 {
                    preimage,
                    claim_id,
                    offer_id,
                    amount,
                    quantity,
                    status,
                    note,
                    payer_note,
                    payer_name,
                    created_at,
                    finalized_at,
                }
            };

            (
                preimage,
                claim_id,
                offer_id,
                amount,
                quantity,
                status,
                note,
                payer_note,
                payer_name,
                created_at,
                finalized_after,
            )
                .prop_map(gen_iip)
                .boxed()
        }
    }

    impl Arbitrary for InboundSpontaneousPaymentV1 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let preimage = any::<LxPaymentPreimage>();
            let amount = any::<Amount>();
            let status =
                any_with::<InboundSpontaneousPaymentStatus>(pending_only);
            let note = any_option_simple_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = any_duration();

            (preimage, amount, status, note, created_at, finalized_after)
                .prop_map(
                    move |(
                        preimage,
                        amount,
                        status,
                        note,
                        created_at,
                        finalized_after,
                    )| {
                        let created_at: TimestampMs = created_at; // provides type hint
                        let finalized_at = if pending_only {
                            None
                        } else {
                            let finalized_at =
                                created_at.saturating_add(finalized_after);
                            PaymentStatus::from(status)
                                .is_finalized()
                                .then_some(finalized_at)
                        };

                        InboundSpontaneousPaymentV1 {
                            hash: preimage.compute_hash(),
                            preimage,
                            amount,
                            // TODO(phlip9): it looks like we don't implement
                            // this yet
                            onchain_fees: None,
                            status,
                            note,
                            created_at,
                            finalized_at,
                        }
                    },
                )
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use arbitrary::gen_values;
    use common::{rng::FastRng, test_utils::snapshot};
    use proptest::arbitrary::any;

    use super::*;
    use crate::payments::v1::PaymentV1;

    #[test]
    fn inbound_offer_reusable_deser_compat() {
        let inputs = r#"
--- node-v0.7.8+ (added reusable inbound offer payments)
--- Claiming
{"InboundOfferReusable":{"claim_id":"ee937f93c40da447b849274371cfe3455074b44e086999ee346105a185a65c36","offer_id":"f2106017b82ff71cd2fdeb0d12b25044ad062dd645b29b013e1f5362ff7e8c2d","preimage":"31fd7e8e51ce64bbe3e8afe4623c7ee648e8195e48dcae073ef42b12c2bfb793","amount":"76041142920849.20","quantity":1,"status":"claiming","note":"KmAm1jofsE64T3lg0dGA1pH9Iio7x70sEZmZu2KaOQa25i1CySWEBtQIpzo5WGZIMiq8B2549ux2M15XNY1PIQYMfUq6f84Gq58xTdLfvGsR0oyio2kyfq57aJuiZOjCO","payer_note":"Mu8BiJSC4A1Z0Q3jOYI3SR9k3eRN1HT1z1eED8QY7m0o4h4wARXjaq5Jq9H","payer_name":"phlip9@lexe.app","created_at":5788173274934005161,"finalized_at":null}}
{"InboundOfferReusable":{"claim_id":"3df77a8027283eb8fca6ef0060adf7d7d26d50f1edb10f8ed8ab092f41abaa0f","offer_id":"9f8e66486e5ced9c3adc2719e4f063987dd9d9b4922379cb6c45a6a18fccc109","preimage":"18855c455c0548c97a914c29e361c204c04d61d359a3d8967af4f4105a5ba85f","amount":"1571893076260348.227","quantity":null,"status":"claiming","note":"CdVQGd0GILKXGI9EBw9BLdLJAstN8oyFPD322E9o39Gvj4613n67zvRyeMaoAHCO3FbhRZKn45N9c3gIc77F59YYDHffsZ2zkwWj7ayJeq5639uCRIwXVvz8L9kF","payer_note":null,"payer_name":null,"created_at":8939796962861345022,"finalized_at":null}}
--- Completed
{"InboundOfferReusable":{"claim_id":"64a97a464679b7c855907bae53113ec098900b7440be9f443b4c0b24f956fe6f","offer_id":"4f38b21130a76e4a4b45ba8bf9a78cc880f5d63823b74502b264128b2f5b9743","preimage":"697605eba6a3f651f559fb2f6a9462bac35bbbe9804f75c2e452df8ea12f3ca6","amount":"986264035966401.277","quantity":123,"status":"completed","note":"w5C2","payer_note":"TCCpwAbfiLHPot2hQT9hvTIj71jF61dIr4","payer_name":"hello@world.com","created_at":398528583856145275,"finalized_at":9223372036854775807}}
{"InboundOfferReusable":{"claim_id":"eb96fda6879dc37b5ac94cd4fb51fcd46207a5419ba8421e28b6e76eef65432b","offer_id":"7b75825b79f00475d020cf434fdc959f0c0e0cdd9f615c721a06f7a4583dbf58","preimage":"001818bfb88429270827996589fdfa0ab71eea380a3cae294ff8071133b57917","amount":"587897171687152.022","quantity":null,"status":"completed","note":"jIsDb3GkqmGSD0XabFkhbNCIo53jaH92A63t8sNR48bh39797pygoJNLd2oINmIyCS6WP3sp5farGwvt44R4YCNgOYRGH3S3RjKYWLBs2nJPv4TsR8H6qg8xinjxD5eFT0amtJw1VDRC3Y83rOgf0b","payer_note":null,"payer_name":null,"created_at":2100409163582470665,"finalized_at":9223372036854775807}}
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let iorp: PaymentV1 = serde_json::from_str(input).unwrap();
            let _ = serde_json::to_string(&iorp).unwrap();
        }
    }

    #[ignore]
    #[test]
    fn inbound_offer_reusable_sample_data() {
        let mut rng = FastRng::from_u64(202504231920);
        let values =
            gen_values(&mut rng, any::<InboundOfferReusablePaymentV1>(), 100);
        for iorp in values {
            let payment = PaymentV1::from(iorp);
            println!("{}", serde_json::to_string(&payment).unwrap());
        }
    }
}
