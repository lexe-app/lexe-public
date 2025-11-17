use std::{num::NonZeroU64, sync::Arc};

use anyhow::ensure;
use common::{ByteArray, ln::amount::Amount, time::TimestampMs};
use lexe_api::types::{
    invoice::LxInvoice,
    offer::LxOffer,
    payments::{
        ClientPaymentId, LxOfferId, LxPaymentHash, LxPaymentId,
        LxPaymentPreimage, LxPaymentSecret,
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

use crate::payments::{PaymentMetadata, PaymentWithMetadata};
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
///
/// [`pay_invoice`]: crate::command::pay_invoice
/// [`PaymentsManager::check_payment_expiries`]: crate::payments::manager::PaymentsManager::check_payment_expiries
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundInvoicePaymentV2 {
    /// The payment hash encoded in the invoice.
    pub hash: LxPaymentHash,
    /// The payment secret encoded in the invoice.
    // BOLT11: "A writer: [...] MUST include exactly one `s` field."
    pub secret: LxPaymentSecret,
    /// The preimage, which serves as a proof-of-payment.
    /// This field is populated if and only if the status is `Completed`.
    pub preimage: Option<LxPaymentPreimage>,

    /// The amount sent in this payment, given by [`Route::get_total_amount`].
    ///
    /// [`Route::get_total_amount`]: lightning::routing::router::Route::get_total_amount
    pub amount: Amount,
    /// The routing fees for this payment. If the payment hasn't completed yet,
    /// this value is only an estimation based on a [`Route`] computed prior to
    /// the first send attempt, as the actual fees paid may vary somewhat due
    /// to retries occurring on different paths. If the payment is
    /// completed, then this field should reflect the actual fees paid.
    ///
    /// [`Route`]: lightning::routing::router::Route
    pub routing_fee: Amount,

    /// The current status of the payment.
    pub status: OutboundInvoicePaymentStatus,

    /// For a failed payment, the reason why it failed.
    // Is part of the core type because (1) it's small and
    // (2) it contains information possibly of interest for later analysis.
    pub failure: Option<LxOutboundPaymentFailure>,

    /// When we initiated this payment.
    /// Set to `Some(...)` on first persist.
    pub created_at: Option<TimestampMs>,
    /// When the invoice expires. Computed from the invoice's timestamp +
    /// expiry duration. `None` if the expiry timestamp overflows.
    pub expires_at: Option<TimestampMs>,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(strum::VariantArray))]
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

impl OutboundInvoicePaymentV2 {
    /// Create a new outbound invoice payment.
    ///
    /// - `amount` is the total amount paid, excluding fees. May be greater than
    ///   the invoiced amount if the payer had to reach `htlc_minimum_msat`
    ///   limits.
    /// - `routing_fee` is the total Lightning routing fees paid.
    //
    // Event sources:
    // - `pay_invoice` API
    pub fn new(
        invoice: LxInvoice,
        amount: Amount,
        routing_fee: Amount,
        note: Option<String>,
    ) -> PaymentWithMetadata<Self> {
        let hash = invoice.payment_hash();
        let secret = invoice.payment_secret();
        let expires_at = invoice.saturating_expires_at();
        let oip = Self {
            hash,
            secret,
            preimage: None,
            amount,
            routing_fee,
            status: OutboundInvoicePaymentStatus::Pending,
            failure: None,
            created_at: None,
            expires_at: Some(expires_at),
            finalized_at: None,
        };

        let metadata = PaymentMetadata {
            id: oip.id(),
            address: None,
            invoice: Some(Arc::new(invoice)),
            offer: None,
            note,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: None,
        };

        PaymentWithMetadata {
            payment: oip,
            metadata,
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

        let estimated_fee = &self.routing_fee;
        let final_routing_fee = maybe_fees_paid
            .inspect(|actual_fee| {
                if actual_fee != estimated_fee {
                    info!(
                        %hash,
                        "Estimated routing fee from Route was {estimated_fee} \
                         msat; actually paid {actual_fee} msat."
                    );
                }
            })
            .unwrap_or_else(|| {
                warn!(
                    "Did not hear back on final routing fee paid for OIP; the \
                    estimated fee will be included with the finalized payment."
                );
                *estimated_fee
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
        clone.routing_fee = final_routing_fee;
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

        // If not expired yet, do nothing.
        let is_expired =
            self.expires_at.is_some_and(|expires_at| expires_at < now);
        if !is_expired {
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
///
/// [`pay_offer`]: crate::command::pay_offer
/// [`PaymentsManager::check_payment_expiries`]: crate::payments::manager::PaymentsManager::check_payment_expiries
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundOfferPaymentV2 {
    /// The unique idempotency id for this payment.
    pub client_id: ClientPaymentId,
    /// The payment hash encoded in the BOLT12 invoice. Since we don't fetch
    /// the BOLT12 invoice before registering the offer payment, this field
    /// is populated iff. the status is `Completed`.
    pub hash: Option<LxPaymentHash>,
    /// The payment preimage, which serves as proof-of-payment.
    /// This field is populated iff. the status is `Completed`.
    pub preimage: Option<LxPaymentPreimage>,
    /// Unique identifier for the original offer.
    pub offer_id: LxOfferId,

    /// The amount sent in this payment excluding fees. May be greater than the
    /// intended value to meet htlc min. limits along the route.
    pub amount: Amount,

    /// The routing fees paid for this payment. If the payment hasn't completed
    /// yet, then this is just an estimate based on the preflight route.
    pub routing_fee: Amount,

    /// The current status of the payment.
    pub status: OutboundOfferPaymentStatus,

    /// For a failed payment, the reason why it failed.
    // Is part of the core type because (1) it's small and
    // (2) it contains information possibly of interest for later analysis.
    pub failure: Option<LxOutboundPaymentFailure>,

    /// When we initiated this payment.
    /// Set to `Some(...)` on first persist.
    pub created_at: Option<TimestampMs>,
    /// When the offer expires. `None` if the offer has no absolute_expiry.
    pub expires_at: Option<TimestampMs>,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::VariantArray, Hash))]
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

impl OutboundOfferPaymentV2 {
    /// Create a new outbound invoice payment.
    ///
    /// - `amount` is the total amount paid, excluding fees. May be greater than
    ///   the invoiced amount if the payer had to reach `htlc_minimum_msat`
    ///   limits.
    /// - `routing_fee` is (currently) an underestimate of the total Lightning
    ///   routing fees paid, since we can't completely route the payment before
    ///   actually fetching the BOLT12 Invoice. Instead these are only the fees
    ///   required to reach last public node on the route, before the blinded
    ///   hops.
    //
    // Event sources:
    // - `pay_offer` API
    pub fn new(
        client_id: ClientPaymentId,
        offer: LxOffer,
        amount: Amount,
        quantity: Option<NonZeroU64>,
        routing_fee: Amount,
        note: Option<String>,
        payer_name: Option<String>,
        payer_note: Option<String>,
    ) -> PaymentWithMetadata<Self> {
        let offer_id = offer.id();
        let expires_at = offer.expires_at();
        let oop = Self {
            client_id,
            hash: None,
            preimage: None,
            offer_id,
            amount,
            routing_fee,
            status: OutboundOfferPaymentStatus::Pending,
            failure: None,
            created_at: None,
            expires_at,
            finalized_at: None,
        };

        let metadata = PaymentMetadata {
            id: oop.id(),
            address: None,
            invoice: None,
            offer: Some(Arc::new(offer)),
            note,
            payer_name,
            payer_note,
            priority: None,
            quantity,
            replacement_txid: None,
        };

        PaymentWithMetadata {
            payment: oop,
            metadata,
        }
    }

    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::OfferSend(self.client_id)
    }

    #[inline]
    pub fn ldk_id(&self) -> lightning::ln::channelmanager::PaymentId {
        lightning::ln::channelmanager::PaymentId(self.client_id.0)
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
        use OutboundOfferPaymentStatus::*;

        let computed_hash = preimage.compute_hash();
        ensure!(hash == computed_hash, "Preimage doesn't correspond to hash");

        // TODO(phlip9): LDK-v0.2 adds `amount_msat` to `PaymentSent` event,
        // which we should use to get the _actual_ amount sent for this offer.

        let estimated_fee = &self.routing_fee;
        let final_routing_fee = maybe_fees_paid
            .inspect(|actual_fee| {
                if actual_fee != estimated_fee {
                    info!(
                        %hash,
                        "Estimated routing fee from Route was {estimated_fee} \
                         msat; actually paid {actual_fee} msat."
                    );
                }
            })
            .unwrap_or_else(|| {
                warn!(
                    "Did not hear back on final routing fee paid for OOP; the \
                     estimated fee will be included with the finalized payment."
                );
                *estimated_fee
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
        clone.routing_fee = final_routing_fee;
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

        // If not expired yet, do nothing.
        let is_expired =
            self.expires_at.is_some_and(|expires_at| expires_at < now);
        if !is_expired {
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

        // Validation complete; offer newly expired

        let mut clone = self.clone();
        clone.status = Abandoning;

        Ok(clone)
    }
}

// --- Outbound spontaneous payments --- //

/// An outbound spontaneous (`keysend`) payment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutboundSpontaneousPaymentV2 {
    /// The hash of this payment.
    pub hash: LxPaymentHash,
    /// The preimage used in this payment, which is generated by us, must match
    /// the hash of this payment, and which must be globally unique to ensure
    /// that intermediate nodes cannot steal funds.
    pub preimage: LxPaymentPreimage,

    /// The amount sent in this payment excluding fees.
    pub amount: Amount,
    /// The routing fees paid for this payment.
    pub routing_fee: Amount,

    /// The current status of the payment.
    pub status: OutboundSpontaneousPaymentStatus,

    /// When we initiated this payment.
    /// Set to `Some(...)` on first persist.
    pub created_at: Option<TimestampMs>,
    /// When this payment either `Completed` or `Failed`.
    pub finalized_at: Option<TimestampMs>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum OutboundSpontaneousPaymentStatus {
    /// We initiated the payment with `send_spontaneous_payment`.
    // TODO(max): Actually implement sending spontaneous payments
    Pending,
    /// We received a [`PaymentSent`] event.
    Completed,
    /// We received a [`PaymentFailed`] event.
    Failed,
}

impl OutboundSpontaneousPaymentV2 {
    #[inline]
    pub fn id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.hash)
    }
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
pub(crate) mod arbitrary_impl {
    use common::test_utils::arbitrary;
    use lexe_api::types::{
        invoice::arbitrary_impl::LxInvoiceParams, payments::LxPaymentPreimage,
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    #[derive(Default)]
    pub struct OipParamsV2 {
        /// Whether to override the payment preimage to this value.
        pub payment_preimage: Option<LxPaymentPreimage>,
        /// Whether to only generate pending payments.
        pub pending_only: bool,
    }

    impl Arbitrary for OutboundInvoicePaymentV2 {
        type Parameters = OipParamsV2;
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
            let routing_fee = any::<Amount>();
            let failure = any::<LxOutboundPaymentFailure>();
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_oip = move |(
                status,
                preimage_invoice,
                amount,
                routing_fee,
                failure_val,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )| {
                use OutboundInvoicePaymentStatus::*;
                let (preimage, invoice): (LxPaymentPreimage, LxInvoice) =
                    preimage_invoice;
                let preimage = (status == Completed).then_some(preimage);
                let hash = invoice.payment_hash();
                let secret = invoice.payment_secret();
                let expires_at = invoice.saturating_expires_at();
                let failure = (status == Failed).then_some(failure_val);

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed | Failed)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed | Failed))
                };

                OutboundInvoicePaymentV2 {
                    hash,
                    secret,
                    preimage,
                    amount,
                    routing_fee,
                    status,
                    failure,
                    created_at,
                    expires_at: Some(expires_at),
                    finalized_at,
                }
            };

            (
                status,
                preimage_invoice,
                amount,
                routing_fee,
                failure,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )
                .prop_map(gen_oip)
                .boxed()
        }
    }

    impl Arbitrary for OutboundInvoicePaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            use proptest::{prelude::Just, prop_oneof};
            use strum::VariantArray;

            if pending_only {
                prop_oneof![
                    Just(OutboundInvoicePaymentStatus::Pending),
                    Just(OutboundInvoicePaymentStatus::Abandoning),
                ]
                .boxed()
            } else {
                proptest::sample::select(OutboundInvoicePaymentStatus::VARIANTS)
                    .boxed()
            }
        }
    }

    impl Arbitrary for OutboundOfferPaymentV2 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let status = any_with::<OutboundOfferPaymentStatus>(pending_only);
            let client_id = any::<ClientPaymentId>();
            let preimage = any::<LxPaymentPreimage>();
            let offer_id = any::<LxOfferId>();

            let amount = any::<Amount>();
            let routing_fee = any::<Amount>();
            let failure = any::<LxOutboundPaymentFailure>();
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let expires_at = any::<Option<TimestampMs>>();
            let finalized_after = arbitrary::any_duration();

            let gen_oop = move |(
                status,
                client_id,
                preimage,
                offer_id,
                amount,
                routing_fee,
                failure,
                maybe_created_at,
                created_at_fallback,
                expires_at,
                finalized_after,
            )| {
                use OutboundOfferPaymentStatus::*;
                let preimage: LxPaymentPreimage = preimage;
                let hash = matches!(status, Completed | Failed)
                    .then_some(preimage.compute_hash());
                let preimage = (status == Completed).then_some(preimage);
                let failure = (status == Failed).then_some(failure);

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed | Failed)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed | Failed))
                };

                OutboundOfferPaymentV2 {
                    client_id,
                    hash,
                    preimage,
                    offer_id,
                    amount,
                    routing_fee,
                    status,
                    failure,
                    created_at,
                    expires_at,
                    finalized_at,
                }
            };

            (
                status,
                client_id,
                preimage,
                offer_id,
                amount,
                routing_fee,
                failure,
                maybe_created_at,
                created_at_fallback,
                expires_at,
                finalized_after,
            )
                .prop_map(gen_oop)
                .boxed()
        }
    }

    impl Arbitrary for OutboundOfferPaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            use proptest::{prelude::Just, prop_oneof};
            use strum::VariantArray;

            if pending_only {
                prop_oneof![
                    Just(OutboundOfferPaymentStatus::Pending),
                    Just(OutboundOfferPaymentStatus::Abandoning),
                ]
                .boxed()
            } else {
                proptest::sample::select(OutboundOfferPaymentStatus::VARIANTS)
                    .boxed()
            }
        }
    }

    impl Arbitrary for OutboundSpontaneousPaymentV2 {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            let status =
                any_with::<OutboundSpontaneousPaymentStatus>(pending_only);
            let preimage = any::<LxPaymentPreimage>();
            let amount = any::<Amount>();
            let routing_fee = any::<Amount>();
            let maybe_created_at = any::<Option<TimestampMs>>();
            let created_at_fallback = any::<TimestampMs>();
            let finalized_after = arbitrary::any_duration();

            let gen_osp = move |(
                status,
                preimage,
                amount,
                routing_fee,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )| {
                use OutboundSpontaneousPaymentStatus::*;

                let preimage: LxPaymentPreimage = preimage; // provides type hint
                let hash = preimage.compute_hash();

                // If finalized, ensure created_at and finalized_at are set
                let maybe_created_at: Option<TimestampMs> = maybe_created_at;
                let created_at = matches!(status, Completed | Failed)
                    .then(|| maybe_created_at.unwrap_or(created_at_fallback));

                let finalized_at = if pending_only {
                    None
                } else {
                    created_at
                        .map(|ts| ts.saturating_add(finalized_after))
                        .filter(|_| matches!(status, Completed | Failed))
                };

                OutboundSpontaneousPaymentV2 {
                    hash,
                    preimage,
                    amount,
                    routing_fee,
                    status,
                    created_at,
                    finalized_at,
                }
            };

            (
                status,
                preimage,
                amount,
                routing_fee,
                maybe_created_at,
                created_at_fallback,
                finalized_after,
            )
                .prop_map(gen_osp)
                .boxed()
        }
    }

    impl Arbitrary for OutboundSpontaneousPaymentStatus {
        // pending_only: whether to only generate pending payments.
        type Parameters = bool;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(pending_only: Self::Parameters) -> Self::Strategy {
            use proptest::prelude::Just;
            use strum::VariantArray;

            if pending_only {
                Just(OutboundSpontaneousPaymentStatus::Pending).boxed()
            } else {
                proptest::sample::select(
                    OutboundSpontaneousPaymentStatus::VARIANTS,
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
