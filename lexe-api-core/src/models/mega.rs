use common::api::{user::UserPk, MegaId};
use serde::{Deserialize, Serialize};

use crate::types::ports::RunPorts;

/// A request to run a usernode within a meganode.
#[derive(Serialize, Deserialize)]
pub struct RunUserRequest {
    /// The user to run.
    pub user_pk: UserPk,

    /// Whether the node should shut down after completing sync.
    pub shutdown_after_sync: bool,

    /// Included to sanity check that we've requested the right meganode.
    pub mega_id: MegaId,
}

#[derive(Serialize, Deserialize)]
pub struct RunUserResponse {
    pub run_ports: RunPorts,
}
