use std::{collections::HashSet, num::NonZeroU64, sync::Arc};

use anyhow::Context;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{ln::amount::Amount, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    payments::{
        LnClaimId, LxOfferId, LxPaymentHash, LxPaymentId, LxPaymentPreimage,
        LxPaymentSecret, PaymentClass,
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
        InboundInvoicePaymentStatus, InboundInvoicePaymentV2,
        InboundOfferReusablePaymentStatus, InboundOfferReusablePaymentV2,
        InboundSpontaneousPaymentStatus, InboundSpontaneousPaymentV2,
    },
};

// --- Inbound invoice payments --- //

/// A 'conventional' inbound payment which is facilitated by an invoice.
/// This struct is created when we call [`create_invoice`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboundInvoicePaymentV1 {
    /// Created in [`create_invoice`].
    pub invoice: Arc<LxInvoice>,
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
            class: PaymentClass::Invoice,
            invoice_amount: v1.invoice_amount,
            recvd_amount: v1.recvd_amount,
            skimmed_fee: None,
            // channel_fee: v1.onchain_fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            expires_at: v1.invoice.expires_at().ok(),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: Some(v1.invoice),
            offer: None,
            note: v1.note,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
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
            class: _,
            invoice_amount,
            recvd_amount,
            skimmed_fee: _,
            // channel_fee: onchain_fees,
            status,
            created_at,
            expires_at: _expires_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            related_ids: _,
            address: _,
            invoice,
            offer: _,
            note,
            payer_name: _,
            payer_note: _,
            priority: _,
            quantity: _,
            replacement_txid: _,
        } = pwm.metadata;

        Ok(Self {
            invoice: invoice.context("Missing invoice")?,
            hash,
            secret,
            preimage,
            claim_id,
            invoice_amount,
            recvd_amount,
            onchain_fees: None,
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
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OfferRecvReusable(self.claim_id)
    }
}

impl From<InboundOfferReusablePaymentV1>
    for PaymentWithMetadata<InboundOfferReusablePaymentV2>
{
    fn from(v1: InboundOfferReusablePaymentV1) -> Self {
        let payment = InboundOfferReusablePaymentV2 {
            claim_id: v1.claim_id,
            offer_id: v1.offer_id,
            preimage: v1.preimage,
            class: PaymentClass::Offer,
            amount: v1.amount,
            skimmed_fee: None,
            // channel_fee: None,
            status: v1.status,
            created_at: Some(v1.created_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: None,
            offer: None,
            note: v1.note,
            payer_name: v1.payer_name,
            payer_note: v1.payer_note,
            priority: None,
            quantity: v1.quantity,
            replacement_txid: None,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<InboundOfferReusablePaymentV2>>
    for InboundOfferReusablePaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<InboundOfferReusablePaymentV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let InboundOfferReusablePaymentV2 {
            claim_id,
            offer_id,
            preimage,
            class: _,
            amount,
            skimmed_fee: _,
            // channel_fee: _,
            status,
            created_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            related_ids: _,
            address: _,
            invoice: _,
            offer: _,
            note,
            payer_name,
            payer_note,
            priority: _,
            quantity,
            replacement_txid: _,
        } = pwm.metadata;

        Ok(Self {
            claim_id,
            offer_id,
            preimage,
            amount,
            quantity,
            status,
            note,
            payer_note,
            payer_name,
            created_at: created_at.context("Missing created_at")?,
            finalized_at,
        })
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
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }
}

impl From<InboundSpontaneousPaymentV1>
    for PaymentWithMetadata<InboundSpontaneousPaymentV2>
{
    fn from(v1: InboundSpontaneousPaymentV1) -> Self {
        let payment = InboundSpontaneousPaymentV2 {
            hash: v1.hash,
            preimage: v1.preimage,
            class: PaymentClass::Spontaneous,
            amount: v1.amount,
            skimmed_fee: None,
            // channel_fee: v1.onchain_fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            related_ids: HashSet::new(),
            address: None,
            invoice: None,
            offer: None,
            note: v1.note,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<InboundSpontaneousPaymentV2>>
    for InboundSpontaneousPaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<InboundSpontaneousPaymentV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let InboundSpontaneousPaymentV2 {
            hash,
            preimage,
            class: _,
            amount,
            skimmed_fee: _,
            // channel_fee: onchain_fees,
            status,
            created_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            related_ids: _,
            address: _,
            invoice: _,
            offer: _,
            priority: _,
            quantity: _,
            replacement_txid: _,
            note,
            payer_note: _,
            payer_name: _,
        } = pwm.metadata;

        Ok(Self {
            hash,
            preimage,
            amount,
            onchain_fees: None,
            status,
            note,
            created_at: created_at.context("Missing created_at")?,
            finalized_at,
        })
    }
}

#[cfg(test)]
mod arb {
    use arbitrary::{any_duration, any_option_simple_string};
    use lexe_api::types::{
        offer::MaxQuantity,
        payments::{LxPaymentPreimage, PaymentStatus},
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for InboundInvoicePaymentV1 {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (
                any::<LxInvoice>(),
                any::<LxPaymentPreimage>(),
                any::<Option<LnClaimId>>(),
                any::<Option<Amount>>(),
                any::<Option<Amount>>(),
                any::<InboundInvoicePaymentStatus>(),
                arbitrary::any_option_simple_string(),
                any::<TimestampMs>(),
                any::<Option<TimestampMs>>(),
            )
                .prop_map(
                    |(
                        invoice,
                        preimage,
                        claim_id,
                        invoice_amount,
                        recvd_amount,
                        status,
                        note,
                        created_at,
                        finalized_at,
                    )| {
                        Self {
                            invoice: Arc::new(invoice.clone()),
                            hash: invoice.payment_hash(),
                            secret: invoice.payment_secret(),
                            preimage,
                            claim_id,
                            invoice_amount,
                            recvd_amount,
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
    fn inbound_offer_reusable_v1_deser_compat() {
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

    #[test]
    fn inbound_invoice_v1_deser_compat() {
        let inputs = r#"
--- node-v0.0.0+
--- InvoiceGenerated
{"InboundInvoice":{"invoice":"lnbc7363509714019145550p1fh8xlrthp5y0ud564d7780074s2pllju2ap7jns0pfqtta9aku6t8mhwvm0j8qpp576k5h3sgt39apacz2ur7k9p50ghhjnvahmtkwuejp2309nrpwsgssp5p3wwm4cmm3a3j6uwaayhuawqf4lf290md8wjmdhpse8p9785sa3q9qyysgqcqrkyf3yxm6n8qklnqh4e3vud7wnx3rp3759up5kc3dulnvz84a0ws25qp0kh8ayymarg8qjn2cawytgztul68vf8s6zscu4x5jfpu03du2qsql3n0ea","hash":"f6ad4bc6085c4bd0f7025707eb14347a2f794d9dbed76773320aa2f2cc617411","secret":"0c5cedd71bdc7b196b8eef497e75c04d7e9515fb69dd2db6e1864e12f8f48762","preimage":"3338296898b6b57ff4bd3526977fd6bc433e5678779334bc4720239fa34214d4","invoice_amount":"736350971401914.555","recvd_amount":null,"onchain_fees":null,"status":"invoice_generated","note":null,"created_at":2395485827019270500,"finalized_at":null}}
--- Claiming
{"InboundInvoice":{"invoice":"lnbcrt14315814875280385750p1jjk2hgxhp59f9glq0mx9xw3yvec466jjrd445rxgefj4h72agwfglale6fkl3spp5q8g0z3rpe2f6tgenkc8e0yymfdvdppt9fvsjj4kweytpz49lvzwssp5e3jdlpgvqm5jy8ffarzp4a8qgqzyrantvefdn0p9navpn88dz82q9q2sqqqqqysgqcqrr48r9yqfteetvkhuv5pc2un4605raj7zdvn055vkkpmpsvl24s04mzcvleg2m6z6mjqw93ndyaw3ufq8j8t2hkkgp60tuhjmh54h2mxygrcyquapr40gs43rad3k3thjnymarjndcl3hhwrvp4dk3vg5027gw4s6z28t596upqmrvl3n0k6hu97p97lhsu73k4jt7yn6wqmcjjxaagggeg8h2s0jxvkgx3qfgzztzuvcsn6y7je33awak9kdsmtlj29aqf25rggt582cc8tx83r0qlnwef6etnzfws9zffd7553yxl2azn5k89fwqj4a4f7tgn5xflm2q60d6zth32h5v9yaa8qyzezgmsfkn7g44gzmn9km7scd0jgm4etgpkhcq2u","hash":"01d0f14461ca93a5a333b60f97909b4b58d085654b212956cec9161154bf609d","secret":"cc64df850c06e9221d29e8c41af4e0400441f66b6652d9bc259f58199ced11d4","preimage":"780a5c91bb7dc7e6dc531cc6fc5560108e00a41b26cb4c5635fffea620589cf6","invoice_amount":"1431581487528038.575","recvd_amount":"631803834701528.778","onchain_fees":null,"status":"claiming","note":null,"created_at":1543439437847952694,"finalized_at":null}}
--- Expired
{"InboundInvoice":{"invoice":"lntb14e0n6q4dz9xumhjup3x9j9xw26wpq5563s2dzkcdm0wuck5v35wfk8qj6kxsurz3m9xfg8yj2rwserzpp5yzwmvkcq55hdfrvjhptswwzgpw0lx9jj7s6pwpsp8pgsd885sspqsp5rzvqgh4e767pj5sw82qdy5a8hha92j8wmaa5khtjt2jype525qvs9qyysgqcqre32r9yq2y0jqz9nstk27c7khlytgt8tvffelnxmv3390uc9k9wl487p20sxew42s3m0hq8hpegg3tr5u53n5qdsypndt8h348355z546tprdkn94hlxdgrp9ggnsksqa7e96tl38k8rdggjxhykujewj6u2auydhc5r3dctfvsr4fmq4cj9hjqdgfykv4eqeujlgldu5tlkzwm3zg8gdm67kr6p8hhy63kwt85rxga2ktu4lkmzkf222udt44y37utqrkfe206wlyyu3sq285nms","hash":"209db65b00a52ed48d92b8570738480b9ff31652f4341706013851069cf48402","secret":"1898045eb9f6bc19520e3a80d253a7bdfa5548eedf7b4b5d725aa440e68aa019","preimage":"11ecd0c5af67c11fd03036c91b30a95db2ec97b2dd2ac4b8da39865215ed745a","invoice_amount":null,"recvd_amount":"1549527423313541.737","onchain_fees":null,"status":"expired","note":null,"created_at":5209058120350254120,"finalized_at":9223372036854775807}}
--- Completed
{"InboundInvoice":{"invoice":"lntbs575933122507938450p17zdsk3uhp5hkwcx7t29pmgr9a9c2qapr994ag7fn920mz7zvs98nu20wzcgv5spp5yuq7s8fl6j56vga6806en6zvwyq28xyfx75y0v5dtuf9rau3h4gqsp5l4r892flu83tmc5apkvyrcsz5ems5222wc6wyq5m35kmxgx32kss9qyysgqcqypu4jxq8lllllllr9yqgz2dq9lhhxfau7kq0gdvm2trf0kf8th9va2flzrxvcjrwfep0zvaajemp576zdx2jhdktt3cxravhqa5qpjmfdwf49g3vakzf64t0ulppsx5c058rmsmeprtjyq7h976r4grn2mpa03xcp42yw655h4cz9pcauwcrjs6pac02yjg0hy4dy7k6eekd5vpv423u70ypp738zc8m3ze7m56d255vn96n5dugkmww32adexzx9kvk9hy8s46ngx8f6dxc4mxvas5vgptckxsl","hash":"2701e81d3fd4a9a623ba3bf599e84c7100a3988937a847b28d5f1251f791bd50","secret":"fd4672a93fe1e2bde29d0d9841e202a6770a294a7634e2029b8d2db320d155a1","preimage":"1e444fb7d12ca78ef4028adc85fd0e50f4ad51a8c12df6362a68fad4e5f60d39","invoice_amount":"57593312250793.845","recvd_amount":"57593312250793.845","onchain_fees":null,"status":"completed","note":"ZTCC2PqaX1yiZNOhvyaF618obYh0c3lGX3G5aAMf0a87pw420f4O078RKAn53C2E1hMKc1b","created_at":7040449765819823150,"finalized_at":9223372036854775807}}

--- node-v0.7.6+ (added `claim_id`)
--- InvoiceGenerated
{"InboundInvoice":{"invoice":"lnbcrt18u4v0srhp5wvppc0lzl5hwytyjnkrv2qt9wwd04mhq4dsszk0q4a4z08my8knqpp5w603ulvghptxqaye9kdpcwlc3gr8cgtz5f2cws98vz56r4g2em0qsp57jvfzy0quadxwx3hux08kjnnfjlm0s2kvxp85q4v96pmhj84mzgs9q2sqqqqqysgqcqypxw4xq8lllllllfppj85qaamls8cyxwrecnq9t9aq9m8zkmlvdrzjqwvdsp6qwz0ftva6adlm4gr8e4kfskr44ww6ptccszlwn306znm6jw3209q0jzdkmpk2npd2r2jk7l4jkcmz9smrd76hmrvrzrtazawttxh7n9ey6ga52edl5nfsaa2eyzfscm9hejlnqqrw64cuwdd2lu76fjcdhgpnlxnd57wzkkz5vv4eaexup4n6exhqhfuz29mv9d4dar0n39cx35w5lj6sfyd5jlpdml4879sq33na6g2gz9luc5xcjyp8sq09jk99","hash":"769f1e7d88b8566074992d9a1c3bf88a067c2162a2558740a760a9a1d50acede","secret":"f4989111e0e75a671a37e19e7b4a734cbfb7c15661827a02ac2e83bbc8f5d891","preimage":"1988fb2aab608204d17d080fc1d76d85d5f531798806a8e86b74f5389ed181fc","claim_id":null,"invoice_amount":null,"recvd_amount":null,"onchain_fees":null,"status":"invoice_generated","note":null,"created_at":5241944617002661841,"finalized_at":null}}
--- Claiming
{"InboundInvoice":{"invoice":"lntbs1ae0a83xhp5au9297ru6q4xchg4lac28kye2g4q8hkgj7zvzyrrvsz66uwuxqcqpp5z72avxg3x8neawwdytn7ku8g66ce79lvh3gzpd8lewg2pc4xdv3ssp5yn055esfwlahzcuaauw9kl5vrl4yzqj43s2zax4ly0fd7umkvnvq9qyysgqcqr9ttxq8lllllllfppj0t2jszfk3jtgjywd88v9plh5alcw950xrw8a4xcskq7xfvez42ynpkzgp5l92uc4dfjpukt77vsnnnk4wfqhxc3scd6jeuskzgwsecgtmzlvzducs7elggadq00vx6ltxujjxgqqkxw9es","hash":"1795d6191131e79eb9cd22e7eb70e8d6b19f17ecbc5020b4ffcb90a0e2a66b23","secret":"24df4a660977fb71639def1c5b7e8c1fea4102558c142e9abf23d2df737664d8","preimage":"0884a7153b88f6d08ec3dde69194176ac5ad2603caa6d66b9f2ffc827ae612b6","claim_id":"57f343039ffcea30c88299a724004ca22d3768256cc270448e97e95aab21a5ca","invoice_amount":null,"recvd_amount":"2075834311210901.218","onchain_fees":null,"status":"claiming","note":"0ewI4M536oYousi883jcreYK16HR7TI0YD7SmWEewDy45E19o56DKXo4BfUE4xo6F9ujLzP8Su9BSloA06RlP3Jr3MpT3U","created_at":7029197943395314647,"finalized_at":null}}
--- Expired
{"InboundInvoice":{"invoice":"lnbc1n24v3n2d258ye4su33ff4rsjr0tgm45vr5fd25sjenwaz9gdmjw9cnzazg2eax2e6twprnyve4fejx24rcwp2rzdes23unzdektym8yjjrdpqkz4mng98ygkn4v9yn2dr3wfeywdfs2ym8xne4fqe42u2kxfhkgc2x8p95cv6nw56hsmfsve9njjpkdd5yujzh2qmnwmtzxs68gjz22fdx6n2s8q64ya20xftxjnp5tgunwen0d924gwr8gaynxa2ktpnnw4jgxat9q4p5g9h45u6s2efy6vn4t9cngdrh8pmhgdfk8pnky5628qukwdttx3jrwvrv89r8yvtkgve42necd9aqpp53utvcugefqkf60yzf3004n8s7rkc0cwsdqgypjumt62g0hsys4kssp59akhrcy7prcy5hx296gmwjh7xjyc48ke49ml2vczaz7ck8m8853s9qyysgqcqypa2sxq8lllllllnp4qvlcj2ex0xlfq0pa7sxh9ezu5uk4q8ke0kcght4z27jsydf3vs3eyr9yqwhs5u4yu5wfnrpdqstlk5m99my0sfheachgcgs7rkunyvhxxse6mzwwy6wy35qqsxadz97f62n266q4espquua7tnr4kfthzp2k88tqap582hpnfshk3sdgq29x97gart8hh4x70w6eqvrakvd2hn7zge7zk5a0hlyqwgjgcct5yxhltqggr8srg4w8v8zpksvk3d4g234nz6ukdh3j0sqjlcnkrt357zk824mulaj32s9gtc8at2hfnssdad8p8up5t7yfpssp5s0s2e","hash":"8f16cc7119482c9d3c824c5efaccf0f0ed87e1d0681040cb9b5e9487de04856d","secret":"2f6d71e09e08f04a5cca2e91b74afe34898a9ed9a977f53302e8bd8b1f673d23","preimage":"f0370488ee0641fbc4b52a8fb84e7936d05e48bad7c8fa46cdbd28679009befd","claim_id":null,"invoice_amount":null,"recvd_amount":null,"onchain_fees":null,"status":"expired","note":null,"created_at":5811472625401499252,"finalized_at":9223372036854775807}}
--- Completed
{"InboundInvoice":{"invoice":"lntb107pekvhdzzv93nxn6w09zrq3zw24t9qdr9x9pnsetytp6nqepeffnx6dn6vah8vnpjfueycmm3wcpp5qcu9tkc4x6usl7jkftumlfcfvqsxyan3q9xqrg3gd8k6kfxkyr3qsp5s05y9zkmkf8xmuh3rwtc5fek8h232420qtqy8u2za8ng7apuffdq9q2sqqqqqysgqcqrap2xq8lllllllmpvkk3knlldp0fagwrnazjt6v6zlt028jcdnml44sh3srlskm7gq72w30wzexjqs4fpael458uav02x9lramk36rjduu20zmp858wwcztv6ulmkgq2t6rg7ptwfyly9m9s33whfq7e3ffjvg8ha7tsqad9c5w","hash":"063855db1536b90ffa564af9bfa7096020627671014c01a22869edab24d620e2","secret":"83e8428adbb24e6df2f11b978a27363dd515554f02c043f142e9e68f743c4a5a","preimage":"26071074a14d199cb7a59b2453376d8f428d226d6bc9649778ec5173a8b65c27","claim_id":"5853c5f40d1836b90f41b491442218df896737303a3bd9c7ef02b41b50b7b764","invoice_amount":null,"recvd_amount":"1184154582399605.725","onchain_fees":null,"status":"completed","note":null,"created_at":1789018149910939233,"finalized_at":9223372036854775807}}
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let iip: PaymentV1 = serde_json::from_str(input).unwrap();
            let _ = serde_json::to_string(&iip).unwrap();
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
