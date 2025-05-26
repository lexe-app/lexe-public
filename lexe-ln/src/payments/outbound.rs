use std::num::NonZeroU64;

use anyhow::ensure;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{ln::amount::Amount, time::TimestampMs, ByteArray};
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
use lightning::{events::PaymentFailureReason, ln::channelmanager::Retry};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[cfg(doc)]
use crate::{
    command::{pay_invoice, pay_offer},
    payments::manager::PaymentsManager,
};

/// The retry strategy we pass to LDK for outbound Lightning payments.
pub const OUTBOUND_PAYMENT_RETRY_STRATEGY: Retry = Retry::Attempts(3);

// --- ExpireError --- //

/// Errors that can occur when expiring an outbound invoice payment.
pub enum ExpireError {
    /// The payment is already finalized or expired. Do nothing.
    Ignore,
    /// The payment was marked to expire. We don't need to persist but we
    /// should re-abandon in case we're coming up after a crash.
    IgnoreAndAbandon,
}

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
    pub note: Option<String>,
    /// When we initiated this payment.
    pub created_at: TimestampMs,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
#[serde(rename_all = "snake_case")]
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
    /// Create a new outbound invoice payment.
    ///
    /// - `amount` is the total amount paid, excluding fees. May be greater than
    ///   the invoiced amount if the payer had to reach `htlc_minimum_msat`
    ///   limits.
    /// - `fees` is the total Lightning routing fees paid.
    //
    // Event sources:
    // - `pay_invoice` API
    pub fn new(
        invoice: LxInvoice,
        amount: Amount,
        fees: Amount,
        note: Option<String>,
    ) -> Self {
        let hash = invoice.payment_hash();
        let secret = invoice.payment_secret();
        Self {
            invoice: Box::new(invoice),
            hash,
            secret,
            preimage: None,
            amount,
            fees,
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

    #[inline]
    pub fn ldk_id(&self) -> lightning::ln::channelmanager::PaymentId {
        lightning::ln::channelmanager::PaymentId(self.hash.to_array())
    }

    /// Handle a [`PaymentSent`] event for this payment.
    ///
    /// ## Precondition
    ///
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentSent` (replayable)
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

        let status = self.status;
        match self.status {
            Pending => (),
            Abandoning =>
                warn!("Attempted to abandon this OIP but it succeeded anyway"),
            Completed | Failed => {
                let id = LxPaymentId::Lightning(hash);
                unreachable!(
                    "caller ensures payment is not already finalized. \
                     {id} is already {status:?}"
                );
            }
        }

        let mut clone = self.clone();
        clone.preimage = Some(preimage);
        clone.fees = final_fees;
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Handle a [`PaymentFailed`] event for this payment.
    ///
    /// ## Precondition
    ///
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentFailed` (replayable)
    // - `pay_invoice` API
    pub(crate) fn check_payment_failed(
        &self,
        id: LxPaymentId,
        failure: LxOutboundPaymentFailure,
    ) -> anyhow::Result<Self> {
        use OutboundInvoicePaymentStatus::*;

        ensure!(
            matches!(id, LxPaymentId::Lightning(hash) if hash == self.hash),
            "Id doesn't match hash",
        );

        let status = self.status;
        match status {
            Pending | Abandoning => (),
            Completed | Failed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}"
            ),
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
    /// ## Precondition
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `PaymentsManager::spawn_payment_expiry_checker` task
    pub(crate) fn check_invoice_expiry(
        &self,
        now: TimestampMs,
    ) -> Result<Self, ExpireError> {
        use OutboundInvoicePaymentStatus::*;

        // Not expired yet, do nothing.
        if !self.invoice.is_expired_at(now) {
            return Err(ExpireError::Ignore);
        }

        match self.status {
            Pending => (),
            // We may crash after persisting the payment but before the channel
            // manager persists. Don't persist anything new, but re-abandon the
            // payment.
            Abandoning => return Err(ExpireError::IgnoreAndAbandon),
            Completed | Failed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status,
            ),
        }

        // Validation complete; invoice newly expired

        let mut clone = self.clone();
        clone.status = Abandoning;

        Ok(clone)
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
pub struct OutboundOfferPayment {
    /// The unique idempotency id for this payment.
    pub cid: ClientPaymentId,
    /// The offer we're paying.
    // LxOffer is ~568 bytes, Box to avoid the enum variant lint
    pub offer: Box<LxOffer>,
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

impl OutboundOfferPayment {
    /// Create a new outbound invoice payment.
    ///
    /// - `amount` is the total amount paid, excluding fees. May be greater than
    ///   the invoiced amount if the payer had to reach `htlc_minimum_msat`
    ///   limits.
    /// - `fees` is (currently) an underestimate of the total Lightning routing
    ///   fees paid, since we can't completely route the payment before actually
    ///   fetching the BOLT12 Invoice. Instead these are only the fees required
    ///   to reach last public node on the route, before the blinded hops.
    //
    // Event sources:
    // - `pay_offer` API
    pub fn new(
        cid: ClientPaymentId,
        offer: LxOffer,
        amount: Amount,
        quantity: Option<NonZeroU64>,
        fees: Amount,
        note: Option<String>,
    ) -> Self {
        Self {
            cid,
            offer: Box::new(offer),
            hash: None,
            preimage: None,
            amount,
            quantity,
            fees,
            note,
            status: OutboundOfferPaymentStatus::Pending,
            failure: None,
            created_at: TimestampMs::now(),
            finalized_at: None,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OfferSend(self.cid)
    }

    #[inline]
    pub fn ldk_id(&self) -> lightning::ln::channelmanager::PaymentId {
        lightning::ln::channelmanager::PaymentId(self.cid.0)
    }

    /// Handle a [`PaymentSent`] event for this payment.
    ///
    /// ## Precondition
    ///
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentSent` (replayable)
    pub fn check_payment_sent(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid: Option<Amount>,
    ) -> anyhow::Result<Self> {
        use OutboundOfferPaymentStatus::*;

        let computed_hash = preimage.compute_hash();
        ensure!(hash == computed_hash, "Preimage doesn't correspond to hash");

        // TODO(phlip9): LDK-v0.2 adds `amount_msat` to `PaymentSent` event,
        // which we should use to get the _actual_ amount sent for this offer.

        let estimated_fees = &self.fees;
        let final_fees = maybe_fees_paid.unwrap_or_else(|| {
            warn!(
                "Did not hear back on final fees paid for OOP; the \
                    estimated fee will be included with the finalized payment."
            );
            *estimated_fees
        });

        let status = self.status;
        match self.status {
            Pending => (),
            Abandoning =>
                warn!("Attempted to abandon this OOP but it succeeded anyway"),
            Completed | Failed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {} is already {status:?}",
                self.id(),
            ),
        }

        let mut clone = self.clone();
        clone.hash = Some(hash);
        clone.preimage = Some(preimage);
        clone.fees = final_fees;
        clone.status = Completed;
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Handle a [`PaymentFailed`] event for this payment.
    ///
    /// ## Precondition
    ///
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `EventHandler` -> `Event::PaymentFailed` (replayable)
    // - `pay_offer` API
    pub(crate) fn check_payment_failed(
        &self,
        failure: LxOutboundPaymentFailure,
    ) -> anyhow::Result<Self> {
        use OutboundOfferPaymentStatus::*;

        let status = self.status;
        match status {
            Pending | Abandoning => (),
            Completed | Failed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {} is already {status:?}",
                self.id(),
            ),
        }

        let mut clone = self.clone();
        clone.status = Failed;
        clone.failure = Some(failure);
        clone.finalized_at = Some(TimestampMs::now());

        Ok(clone)
    }

    /// Checks whether this payment's offer has expired. If so, and if the
    /// state transition to `Abandoning` is valid, returns a clone with the
    /// state transition applied.
    ///
    /// ## Precondition
    /// - The payment must not be finalized (Completed | Failed).
    //
    // Event sources:
    // - `PaymentsManager::spawn_payment_expiry_checker` task
    pub(crate) fn check_offer_expiry(
        &self,
        now: TimestampMs,
    ) -> Result<Self, ExpireError> {
        use OutboundOfferPaymentStatus::*;

        // Not expired yet, do nothing.
        if !self.offer.is_expired_at(now) {
            return Err(ExpireError::Ignore);
        }

        match self.status {
            Pending => (),
            // We may crash after persisting the payment but before the channel
            // manager persists. Don't persist anything new, but re-abandon the
            // payment.
            Abandoning => return Err(ExpireError::IgnoreAndAbandon),
            Completed | Failed => unreachable!(
                "caller ensures payment is not already finalized. \
                 {id} is already {status:?}",
                id = self.id(),
                status = self.status,
            ),
        }

        // Validation complete; invoice newly expired

        let mut clone = self.clone();
        clone.status = Abandoning;

        Ok(clone)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray, Hash))]
pub enum OutboundOfferPaymentStatus {
    /// We initiated this payment with [`pay_offer`].
    Pending,
    /// The offer expired and we called [`ChannelManager::abandon_payment`],
    /// but we haven't yet received a [`PaymentFailed`] (or [`PaymentSent`])
    /// event to finalize the payment.
    Abandoning,
    /// We received a [`PaymentSent`] event.
    Completed,
    /// We received a [`PaymentFailed`] event, or the initial send in
    /// [`pay_offer`] "failed outright".
    Failed,
}

// --- Outbound spontaneous payments --- //

/// An outbound spontaneous (`keysend`) payment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
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
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
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
    /// The payment metadata is too large, causing us to exceed the maximum
    /// onion packet size.
    MetadataTooLarge,
    /// An invoice was received that required unknown features.
    UnknownFeatures,
    /// A BOLT 12 invoice was not received in time.
    InvoiceRequestExpired,
    /// The recipient rejected our BOLT 12 invoice request.
    InvoiceRequestRejected,
    /// Failed to find a reply route from the destination back to us.
    BlindedPathCreationFailed,
    /// Something about the BOLT12 offer was invalid.
    InvalidOffer,
    /// API misuse error. Probably a bug in Lexe code.
    LexeErr,
    /// Any unrecognized variant we might deserialize. This variant is for
    /// forwards compatibility (old node reads new state).
    #[serde(other)]
    Unknown,
}

impl LxOutboundPaymentFailure {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoRetries => "no successful payment after all retry attempts",
            Self::Rejected => "the recipient rejected our payment",
            Self::Abandoned => "the payment was canceled",
            Self::Expired =>
                "the invoice expired before we could complete the payment",
            Self::NoRoute => "could not find usable route to send payment over",
            Self::MetadataTooLarge => "invalid payment metadata: too large",
            Self::UnknownFeatures => "invoice requires unknown features",
            Self::InvoiceRequestExpired =>
                "recipient did not respond with the invoice in time",
            Self::InvoiceRequestRejected =>
                "recipient rejected our invoice request",
            Self::BlindedPathCreationFailed =>
                "failed to find a reply route back to us",
            Self::InvalidOffer => "invalid offer",
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
            UnknownRequiredFeatures => Self::UnknownFeatures,
            InvoiceRequestExpired => Self::InvoiceRequestExpired,
            InvoiceRequestRejected => Self::InvoiceRequestRejected,
            BlindedPathCreationFailed => Self::BlindedPathCreationFailed,
        }
    }
}

#[cfg(test)]
pub(crate) mod arb {
    use arbitrary::any_duration;
    use common::{self, test_utils::arbitrary::any_option_string};
    use lexe_api::types::{
        invoice::arbitrary_impl::LxInvoiceParams, payments::LxPaymentPreimage,
    };
    use proptest::{
        arbitrary::{any, any_with, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    #[derive(Default)]
    pub struct OipParams {
        pub payment_preimage: Option<LxPaymentPreimage>,
    }

    impl Arbitrary for OutboundInvoicePayment {
        type Parameters = OipParams;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            let status = any::<OutboundInvoicePaymentStatus>();
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
            let finalized_after = any_duration();

            let gen_oip = |(
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
                let invoice = Box::new(invoice);
                let failure = (status == Failed).then_some(failure);
                let created_at: TimestampMs = created_at;
                let finalized_at = created_at.saturating_add(finalized_after);
                let finalized_at = matches!(status, Completed | Failed)
                    .then_some(finalized_at);
                OutboundInvoicePayment {
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

    impl Arbitrary for OutboundOfferPayment {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let status = any::<OutboundOfferPaymentStatus>();
            let cid = any::<ClientPaymentId>();
            let offer = any::<Box<LxOffer>>();
            let preimage = any::<LxPaymentPreimage>();

            let amount = any::<Amount>();
            let quantity = any::<Option<NonZeroU64>>();
            let fees = any::<Amount>();
            let failure = any::<LxOutboundPaymentFailure>();
            let note = any_option_string();
            let created_at = any::<TimestampMs>();
            let finalized_after = any_duration();

            let gen_oop = |(
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
                let created_at: TimestampMs = created_at;
                let finalized_at = created_at.saturating_add(finalized_after);
                let finalized_at = matches!(status, Completed | Failed)
                    .then_some(finalized_at);
                OutboundOfferPayment {
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

    impl Arbitrary for OutboundSpontaneousPayment {
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
            let finalized_after = any_duration();

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
                OutboundSpontaneousPayment {
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

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip::json_unit_enum_backwards_compat;

    use super::*;

    #[test]
    fn status_json_backward_compat() {
        let expected_ser = r#"["pending","abandoning","completed","failed"]"#;
        json_unit_enum_backwards_compat::<OutboundInvoicePaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["pending","abandoning","completed","failed"]"#;
        json_unit_enum_backwards_compat::<OutboundOfferPaymentStatus>(
            expected_ser,
        );

        let expected_ser = r#"["pending","completed","failed"]"#;
        json_unit_enum_backwards_compat::<OutboundSpontaneousPaymentStatus>(
            expected_ser,
        );
    }

    #[test]
    fn lx_outbound_payment_failure_json_backwards_compat() {
        let expected_ser = r#"["NoRetries","Rejected","Abandoned","Expired","NoRoute","MetadataTooLarge","UnknownFeatures","InvoiceRequestExpired","InvoiceRequestRejected","BlindedPathCreationFailed","InvalidOffer","LexeErr","Unknown"]"#;
        json_unit_enum_backwards_compat::<LxOutboundPaymentFailure>(
            expected_ser,
        );
    }

    // Old nodes will deserialize unrecognized failure variants as `Unknown`
    #[test]
    fn lx_outbound_payment_failure_json_forward_compat() {
        let s = "\"SomeNewVariant\"";
        let expected_de = LxOutboundPaymentFailure::Unknown;
        let actual_de =
            serde_json::from_str::<LxOutboundPaymentFailure>(s).unwrap();
        assert_eq!(actual_de, expected_de);
    }
}
