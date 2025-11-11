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

// --- Outbound offer payments --- //

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

// --- Outbound spontaneous payments --- //

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
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

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
