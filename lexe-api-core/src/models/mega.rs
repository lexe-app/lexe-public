use common::{
    api::{user::UserPk, MegaId},
    time::TimestampMs,
};
use serde::{Deserialize, Serialize};

use crate::types::{ports::RunPorts, LeaseId};

/// A request to run a usernode within a meganode.
#[derive(Serialize, Deserialize)]
pub struct RunUserRequest {
    /// The user to run.
    pub user_pk: UserPk,

    /// The lease ID for this user node.
    pub lease_id: LeaseId,

    /// Included to sanity check that we've requested the right meganode.
    pub mega_id: MegaId,

    /// Whether the node should shut down after completing sync.
    pub shutdown_after_sync: bool,
}

#[derive(Serialize, Deserialize)]
pub struct RunUserResponse {
    pub run_ports: RunPorts,
}

/// A request from a usernode to renew its lease.
// This is technically a usernode request, but it's meganode related so....
#[derive(Serialize, Deserialize)]
pub struct UserLeaseRenewalRequest {
    /// The ID of the lease to renew.
    pub lease_id: LeaseId,
    /// Sanity check: The requesting user.
    pub user_pk: UserPk,
    /// Sanity check: The current time within the enclave.
    pub timestamp: TimestampMs,
}

/// A notification from a meganode that a user has shut down,
/// and that we can terminate the user's lease.
#[derive(Serialize, Deserialize)]
pub struct UserFinishedRequest {
    /// The user that shut down.
    pub user_pk: UserPk,
    /// The ID of the lease to terminate.
    pub lease_id: LeaseId,
    /// Sanity check: The meganode issuing the request.
    pub mega_id: MegaId,
}
