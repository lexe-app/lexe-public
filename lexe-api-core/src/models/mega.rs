use common::api::{user::UserPk, MegaId};
use serde::{Deserialize, Serialize};

/// A request to run a usernode within a meganode.
#[derive(Serialize, Deserialize)]
pub struct RunUserRequest {
    /// The user to run.
    pub user_pk: UserPk,

    /// Included to sanity check that we've requested the right meganode.
    pub mega_id: MegaId,
}

#[derive(Serialize, Deserialize)]
pub struct RunUserResponse {}
