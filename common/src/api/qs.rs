use serde::{Deserialize, Serialize};

use crate::api::{NodePk, Scid, UserPk};
#[cfg(doc)]
use crate::ln::payments::LxPaymentId;
use crate::ln::payments::PaymentIndex;

// When serializing data as query parameters, we have to wrap newtypes in these
// structs (instead of e.g. using UserPk directly), otherwise `serde_qs` errors
// with "top-level serializer supports only maps and structs."

/// Query parameter struct for fetching with no data attached.
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Query parameter struct for fetching by user pk
#[derive(Serialize, Deserialize)]
pub struct GetByUserPk {
    pub user_pk: UserPk,
}

/// Query parameter struct for fetching by node pk
#[derive(Serialize, Deserialize)]
pub struct GetByNodePk {
    pub node_pk: NodePk,
}

/// Query parameter struct for fetching by scid
#[derive(Serialize, Deserialize)]
pub struct GetByScid {
    pub scid: Scid,
}

/// Query parameter struct for syncing batches of new payments to local storage.
/// Results are returned in ascending `(created_at, payment_id)` order.
#[derive(Serialize, Deserialize)]
pub struct GetNewPayments {
    /// Optional [`PaymentIndex`] at which the results should start, exclusive.
    /// Payments with an index less than or equal to this will not be returned.
    #[serde(flatten)]
    pub start_index: Option<PaymentIndex>,
    /// (Optional) the maximum number of results that can be returned.
    pub limit: Option<u16>,
}

/// Struct for fetching payments by [`LxPaymentId`].
// NOTE: This struct isn't actually serialized into query parameters - this
// struct is sent via `POST` instead (and so uses JSON).
#[derive(Serialize, Deserialize)]
pub struct GetPaymentsByIds {
    /// The string-serialized [`LxPaymentId`]s of the payments to be fetched.
    /// Typically, the ids passed here correspond to payments that the mobile
    /// client currently has stored locally as "pending"; the intention is to
    /// check whether any of these payments have been updated.
    pub ids: Vec<String>,
}
