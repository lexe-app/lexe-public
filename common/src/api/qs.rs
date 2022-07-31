use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::Measurement;

/// Query parameter struct for fetching with no data attached
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Query parameter struct for fetching by user pk
#[derive(Deserialize, Serialize)]
pub struct GetByUserPk {
    pub user_pk: UserPk,
}

/// Query parameter struct for fetching by user pk and measurement
#[derive(Serialize)]
pub struct GetByUserPkAndMeasurement {
    pub user_pk: UserPk,
    pub measurement: Measurement,
}
