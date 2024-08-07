#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::ln::payments::LxPaymentId;
use crate::{
    api::{NodePk, Scid, UserPk},
    enclave::Measurement,
    ln::payments::PaymentIndex,
};

// When serializing data as query parameters, we have to wrap newtypes in these
// structs (instead of e.g. using UserPk directly), otherwise `serde_qs` errors
// with "top-level serializer supports only maps and structs."

// Also beware when using `#[serde(flatten)]` on a field. All inner fields must
// be string-ish types (&str, String, Cow<'_, str>, etc...) OR use
// `SerializeDisplay` and `DeserializeFromStr` from `serde_with`.
//
// This issue is due to a limitation in serde. See:
// <https://github.com/serde-rs/serde/issues/1183>

/// Query parameter struct for fetching by user pk
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetByUserPk {
    pub user_pk: UserPk,
}

/// Query parameter struct for fetching by node pk
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetByNodePk {
    pub node_pk: NodePk,
}

/// Query parameter struct for fetching by scid
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetByScid {
    pub scid: Scid,
}

/// Query parameter struct for fetching by measurement.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetByMeasurement {
    pub measurement: Measurement,
}

/// Query parameter struct for fetching a payment by its index.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetPaymentByIndex {
    /// The index of the payment to be fetched.
    // We use index instead of id so the backend can query by primary key.
    pub index: PaymentIndex,
}

/// Query parameter struct for syncing batches of new payments to local storage.
/// Results are returned in ascending `(created_at, payment_id)` order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GetNewPayments {
    /// Optional [`PaymentIndex`] at which the results should start, exclusive.
    /// Payments with an index less than or equal to this will not be returned.
    pub start_index: Option<PaymentIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Struct for fetching payments by [`PaymentIndex`].
// NOTE: This struct isn't actually serialized into query parameters - this
// struct is sent via `POST` instead (and so uses JSON).
#[derive(Serialize, Deserialize)]
pub struct GetPaymentsByIndexes {
    /// The string-serialized [`PaymentIndex`]s of the payments to be fetched.
    /// Typically, the ids passed here correspond to payments that the mobile
    /// client currently has stored locally as "pending"; the intention is to
    /// check whether any of these payments have been updated.
    pub indexes: Vec<PaymentIndex>,
}

/// Struct for updating payment notes.
// TODO(max): Not a query param struct; maybe these structs should be moved...
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdatePaymentNote {
    /// The index of the payment whose note should be updated.
    pub index: PaymentIndex,
    /// The updated note.
    pub note: Option<String>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip::query_string_roundtrip_proptest;

    #[test]
    fn get_by_user_pk_roundtrip() {
        query_string_roundtrip_proptest::<GetByUserPk>();
    }

    #[test]
    fn get_by_node_pk_roundtrip() {
        query_string_roundtrip_proptest::<GetByNodePk>();
    }

    #[test]
    fn get_by_scid_roundtrip() {
        query_string_roundtrip_proptest::<GetByScid>();
    }

    #[test]
    fn get_by_measurement_roundtrip() {
        query_string_roundtrip_proptest::<GetByMeasurement>();
    }

    #[test]
    fn get_payment_by_index_roundtrip() {
        query_string_roundtrip_proptest::<GetPaymentByIndex>();
    }

    #[test]
    fn get_new_payments_roundtrip() {
        query_string_roundtrip_proptest::<GetNewPayments>();
    }
}
