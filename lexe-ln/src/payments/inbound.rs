use std::num::NonZeroU64;

use anyhow::{Context, anyhow};
use lexe_api::types::payments::{
    LnClaimId, LxOfferId, LxPaymentHash, LxPaymentId, LxPaymentPreimage,
    LxPaymentSecret, PaymentKind,
};
use lightning::events::PaymentPurpose;
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::Event::{PaymentClaimable, PaymentClaimed},
    ln::channelmanager::ChannelManager,
};
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::{
    command::create_invoice,
    payments::v1::inbound::InboundOfferReusablePaymentV1,
};

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
/// [`InboundOfferReusablePaymentV1`].
#[derive(Clone)]
pub struct OfferClaimCtx {
    pub preimage: LxPaymentPreimage,
    // We don't have any BOLT12 offers pending, so we can assume claim id
    // is present.
    pub claim_id: LnClaimId,
    pub offer_id: LxOfferId,
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

// --- Inbound BOLT12 offer payments --- //

// TODO(phlip9): single-use BOLT12 offer payments

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

// --- Inbound spontaneous payments --- //

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

#[cfg(test)]
mod arb {

    use proptest::{
        arbitrary::Arbitrary,
        prelude::Just,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };
    use strum::VariantArray;

    use super::*;

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

    use common::test_utils::roundtrip::json_unit_enum_backwards_compat;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser =
            r#"["invoice_generated","claiming","completed","expired"]"#;
        json_unit_enum_backwards_compat::<InboundInvoicePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["claiming","completed"]"#;
        json_unit_enum_backwards_compat::<InboundOfferReusablePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["claiming","completed"]"#;
        json_unit_enum_backwards_compat::<InboundSpontaneousPaymentStatus>(
            expected_ser,
        );
    }
}
