use std::{num::NonZeroU64, sync::Arc};

use anyhow::Context;
use common::{ByteArray, ln::amount::Amount, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        ClientPaymentId, LxPaymentHash, LxPaymentId, LxPaymentPreimage,
        LxPaymentSecret,
    },
};
#[cfg(doc)] // Adding these imports significantly reduces doc comment noise
use lightning::{
    events::Event::{PaymentFailed, PaymentSent},
    events::PaymentPurpose,
    ln::channelmanager::ChannelManager,
    routing::router::Route,
};
use serde::{Deserialize, Serialize};

use crate::payments::{
    PaymentMetadata, PaymentWithMetadata,
    outbound::{
        LxOutboundPaymentFailure, OutboundInvoicePaymentStatus,
        OutboundInvoicePaymentV2, OutboundOfferPaymentStatus,
        OutboundOfferPaymentV2, OutboundSpontaneousPaymentStatus,
        OutboundSpontaneousPaymentV2,
    },
};
#[cfg(doc)]
use crate::{
    command::{pay_invoice, pay_offer},
    payments::manager::PaymentsManager,
};

// --- Outbound invoice payments --- //

/// A 'conventional' outbound payment where we pay an invoice provided to us by
/// our recipient.
///
/// ## Relevant events
///
/// - [`pay_invoice`] API
/// - [`PaymentFailed`] event
/// - [`PaymentSent`] event
/// - [`PaymentsManager::check_payment_expiries`] task
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundInvoicePaymentV1 {
    /// The invoice given by our recipient which we want to pay.
    // LxInvoice is ~300 bytes, Box to avoid the enum variant lint
    pub invoice: Arc<LxInvoice>,
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
    pub note: Option<String>,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

impl OutboundInvoicePaymentV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }

    #[inline]
    pub fn ldk_id(&self) -> lightning::ln::channelmanager::PaymentId {
        lightning::ln::channelmanager::PaymentId(self.hash.to_array())
    }
}

impl From<OutboundInvoicePaymentV1>
    for PaymentWithMetadata<OutboundInvoicePaymentV2>
{
    fn from(v1: OutboundInvoicePaymentV1) -> Self {
        let expires_at = v1.invoice.saturating_expires_at();
        let payment = OutboundInvoicePaymentV2 {
            hash: v1.hash,
            secret: v1.secret,
            preimage: v1.preimage,
            amount: v1.amount,
            routing_fee: v1.fees,
            status: v1.status,
            failure: v1.failure,
            created_at: Some(v1.created_at),
            expires_at: Some(expires_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            address: None,
            invoice: Some(v1.invoice),
            offer: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
            note: v1.note,
            payer_note: None,
            payer_name: None,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<OutboundInvoicePaymentV2>>
    for OutboundInvoicePaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<OutboundInvoicePaymentV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let OutboundInvoicePaymentV2 {
            hash,
            secret,
            preimage,
            amount,
            routing_fee: fees,
            status,
            failure,
            created_at,
            expires_at: _,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            address: _,
            invoice,
            offer: _,
            priority: _,
            quantity: _,
            replacement_txid: _,
            note,
            payer_note: _,
            payer_name: _,
        } = pwm.metadata;

        let invoice = invoice.context("Missing invoice")?;
        let created_at = created_at.context("Missing created_at")?;

        Ok(Self {
            invoice,
            hash,
            secret,
            preimage,
            amount,
            fees,
            status,
            failure,
            note,
            created_at,
            finalized_at,
        })
    }
}

// --- Outbound offer payments --- //

/// An outbound payment for a BOLT12 offer.
///
/// ## Relevant events
///
/// - [`pay_offer`] API
/// - [`PaymentFailed`] event
/// - [`PaymentSent`] event
/// - [`PaymentsManager::check_payment_expiries`] task
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundOfferPaymentV1 {
    /// The unique idempotency id for this payment.
    pub cid: ClientPaymentId,
    /// The offer we're paying.
    // LxOffer is ~568 bytes, Box to avoid the enum variant lint
    pub offer: Arc<LxOffer>,
    /// The payment hash encoded in the BOLT12 invoice. Since we don't fetch
    /// the BOLT12 invoice before registering the offer payment, this field
    /// is populated iff. the status is `Completed`.
    pub hash: Option<LxPaymentHash>,
    /// The payment preimage, which serves as proof-of-payment.
    /// This field is populated iff. the status is `Completed`.
    pub preimage: Option<LxPaymentPreimage>,
    /// The amount sent in this payment excluding fees. May be greater than the
    /// intended value to meet htlc min. limits along the route.
    pub amount: Amount,
    /// The number of "units" purchased.
    pub quantity: Option<NonZeroU64>,
    /// The routing fees paid for this payment. If the payment hasn't completed
    /// yet, then this is just an estimate based on the preflight route.
    pub fees: Amount,
    /// The current status of the payment.
    pub status: OutboundOfferPaymentStatus,
    /// For a failed payment, the reason why it failed.
    pub failure: Option<LxOutboundPaymentFailure>,
    /// An optional personal note for this payment.
    pub note: Option<String>,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

impl OutboundOfferPaymentV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OfferSend(self.cid)
    }

    #[inline]
    pub fn ldk_id(&self) -> lightning::ln::channelmanager::PaymentId {
        lightning::ln::channelmanager::PaymentId(self.cid.0)
    }
}

impl From<OutboundOfferPaymentV1>
    for PaymentWithMetadata<OutboundOfferPaymentV2>
{
    fn from(v1: OutboundOfferPaymentV1) -> Self {
        let offer_id = v1.offer.id();
        let expires_at = v1.offer.expires_at();
        let payment = OutboundOfferPaymentV2 {
            client_id: v1.cid,
            hash: v1.hash,
            preimage: v1.preimage,
            offer_id,
            amount: v1.amount,
            routing_fee: v1.fees,
            status: v1.status,
            failure: v1.failure,
            created_at: Some(v1.created_at),
            expires_at,
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            address: None,
            invoice: None,
            offer: Some(v1.offer),
            priority: None,
            quantity: v1.quantity,
            replacement_txid: None,
            note: v1.note,
            payer_note: None,
            payer_name: None,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<OutboundOfferPaymentV2>>
    for OutboundOfferPaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<OutboundOfferPaymentV2>,
    ) -> Result<Self, Self::Error> {
        // Intentionally destructure to ensure all fields are considered.
        let OutboundOfferPaymentV2 {
            client_id: cid,
            hash,
            preimage,
            offer_id: _,
            amount,
            routing_fee: fees,
            status,
            failure,
            created_at,
            expires_at: _,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
            address: _,
            invoice: _,
            offer,
            priority: _,
            quantity,
            replacement_txid: _,
            note,
            payer_note: _,
            payer_name: _,
        } = pwm.metadata;

        let offer = offer.context("Missing offer")?;
        let created_at = created_at.context("Missing created_at")?;

        Ok(Self {
            cid,
            offer,
            hash,
            preimage,
            amount,
            quantity,
            fees,
            status,
            failure,
            note,
            created_at,
            finalized_at,
        })
    }
}

// --- Outbound spontaneous payments --- //

/// An outbound spontaneous (`keysend`) payment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundSpontaneousPaymentV1 {
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
    pub note: Option<String>,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

impl OutboundSpontaneousPaymentV1 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }
}

impl From<OutboundSpontaneousPaymentV1>
    for PaymentWithMetadata<OutboundSpontaneousPaymentV2>
{
    fn from(v1: OutboundSpontaneousPaymentV1) -> Self {
        let payment = OutboundSpontaneousPaymentV2 {
            hash: v1.hash,
            preimage: v1.preimage,
            amount: v1.amount,
            routing_fee: v1.fees,
            status: v1.status,
            created_at: Some(v1.created_at),
            finalized_at: v1.finalized_at,
        };
        let metadata = PaymentMetadata {
            id: v1.id(),
            address: None,
            invoice: None,
            offer: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
            note: v1.note,
            payer_note: None,
            payer_name: None,
        };

        Self { payment, metadata }
    }
}

impl TryFrom<PaymentWithMetadata<OutboundSpontaneousPaymentV2>>
    for OutboundSpontaneousPaymentV1
{
    type Error = anyhow::Error;

    fn try_from(
        pwm: PaymentWithMetadata<OutboundSpontaneousPaymentV2>,
    ) -> Result<Self, Self::Error> {
        let OutboundSpontaneousPaymentV2 {
            hash,
            preimage,
            amount,
            routing_fee: fees,
            status,
            created_at,
            finalized_at,
        } = pwm.payment;
        let PaymentMetadata {
            id: _,
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

        let created_at = created_at.context("Missing created_at")?;

        Ok(Self {
            hash,
            preimage,
            amount,
            fees,
            status,
            note,
            created_at,
            finalized_at,
        })
    }
}

#[cfg(test)]
pub(crate) mod arb {
    use common::{
        self,
        test_utils::{arbitrary, arbitrary::any_option_string},
    };
    use lexe_api::types::{
        invoice::arbitrary_impl::LxInvoiceParams, payments::LxPaymentPreimage,
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    #[derive(Default)]
    pub struct OipParamsV1 {
        /// Whether to override the payment preimage to this value.
        pub payment_preimage: Option<LxPaymentPreimage>,
        /// Whether to only generate pending payments.
        pub pending_only: bool,
    }

    impl Arbitrary for OutboundInvoicePaymentV1 {
        type Parameters = OipParamsV1;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            let pending_only = args.pending_only;
            let status = any_with::<OutboundInvoicePaymentStatus>(pending_only);
            let preimage =
                any::<LxPaymentPreimage>().prop_map(move |preimage| {
                    args.payment_preimage.unwrap_or(preimage)
                });
            let preimage_invoice = preimage.prop_ind_flat_map2(|preimage| {
                any_with::<LxInvoice>(LxInvoiceParams {
                    payment_preimage: Some(preimage),
                })
            });

            let amount = any::<Amount>();
            let fees = any::<Amount>();
            let failure = any::<LxOutboundPaymentFailure>();
            let note = any_option_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_oip = move |(
                status,
                preimage_invoice,
                amount,
                fees,
                failure,
                note,
                created_at,
                finalized_after,
            )| {
                use OutboundInvoicePaymentStatus::*;
                let (preimage, invoice): (LxPaymentPreimage, LxInvoice) =
                    preimage_invoice;
                let preimage = (status == Completed).then_some(preimage);
                let hash = invoice.payment_hash();
                let secret = invoice.payment_secret();
                let invoice = Arc::new(invoice);
                let failure = (status == Failed).then_some(failure);
                let created_at: TimestampMs = created_at; // provides type hint
                let finalized_at = created_at.saturating_add(finalized_after);
                let finalized_at = matches!(status, Completed | Failed)
                    .then_some(finalized_at);

                OutboundInvoicePaymentV1 {
                    invoice,
                    hash,
                    secret,
                    preimage,
                    amount,
                    fees,
                    status,
                    failure,
                    note,
                    created_at,
                    finalized_at,
                }
            };

            (
                status,
                preimage_invoice,
                amount,
                fees,
                failure,
                note,
                created_at,
                finalized_after,
            )
                .prop_map(gen_oip)
                .boxed()
        }
    }

    impl Arbitrary for OutboundOfferPaymentV1 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let status = any_with::<OutboundOfferPaymentStatus>(pending_only);
            let cid = any::<ClientPaymentId>();
            let offer = any::<LxOffer>().prop_map(Arc::new);
            let preimage = any::<LxPaymentPreimage>();

            let amount = any::<Amount>();
            let quantity = any::<Option<NonZeroU64>>();
            let fees = any::<Amount>();
            let failure = any::<LxOutboundPaymentFailure>();
            let note = any_option_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_oop = move |(
                status,
                cid,
                offer,
                preimage,
                amount,
                quantity,
                fees,
                failure,
                note,
                created_at,
                finalized_after,
            )| {
                use OutboundOfferPaymentStatus::*;
                let preimage: LxPaymentPreimage = preimage;
                let hash = matches!(status, Completed | Failed)
                    .then_some(preimage.compute_hash());
                let preimage = (status == Completed).then_some(preimage);
                let failure = (status == Failed).then_some(failure);
                let created_at: TimestampMs = created_at; // provides type hint
                let finalized_at = created_at.saturating_add(finalized_after);
                let finalized_at = matches!(status, Completed | Failed)
                    .then_some(finalized_at);

                OutboundOfferPaymentV1 {
                    cid,
                    offer,
                    hash,
                    preimage,
                    amount,
                    quantity,
                    fees,
                    status,
                    failure,
                    note,
                    created_at,
                    finalized_at,
                }
            };

            (
                status,
                cid,
                offer,
                preimage,
                amount,
                quantity,
                fees,
                failure,
                note,
                created_at,
                finalized_after,
            )
                .prop_map(gen_oop)
                .boxed()
        }
    }

    impl Arbitrary for OutboundSpontaneousPaymentV1 {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            use OutboundSpontaneousPaymentStatus::*;
            let preimage = any::<LxPaymentPreimage>();
            let amount = any::<Amount>();
            let fees = any::<Amount>();
            let status = any::<OutboundSpontaneousPaymentStatus>();
            let note = any_option_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_osp = |(
                preimage,
                amount,
                fees,
                status,
                note,
                created_at,
                finalized_after,
            )| {
                let preimage: LxPaymentPreimage = preimage;
                let hash = preimage.compute_hash();
                let created_at: TimestampMs = created_at;
                let finalized_at = matches!(status, Completed | Failed)
                    .then_some(created_at.saturating_add(finalized_after));
                OutboundSpontaneousPaymentV1 {
                    hash,
                    preimage,
                    amount,
                    fees,
                    status,
                    note,
                    created_at,
                    finalized_at,
                }
            };

            (
                preimage,
                amount,
                fees,
                status,
                note,
                created_at,
                finalized_after,
            )
                .prop_map(gen_osp)
                .boxed()
        }
    }
}
