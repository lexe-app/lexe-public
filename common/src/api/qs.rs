use serde::{Deserialize, Serialize};

use crate::api::UserPk;

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
