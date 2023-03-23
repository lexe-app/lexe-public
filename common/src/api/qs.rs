use serde::{Deserialize, Serialize};

use crate::api::{NodePk, Scid, UserPk};
use crate::time::TimestampMs;

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

/// Fetch a range of timestamped items.
#[derive(Serialize, Deserialize)]
pub struct GetRange {
    /// The start of the range, inclusive.
    pub start: Option<TimestampMs>,
    /// The end of the range, exclusive.
    pub end: Option<TimestampMs>,
}
