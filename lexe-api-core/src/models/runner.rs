use common::{
    api::{MegaId, user::UserPk},
    time::TimestampMs,
};
use serde::{Deserialize, Serialize};

use crate::types::{LeaseId, ports::RunPorts};

/// A request sent to a meganode API server to run a usernode within a meganode.
#[derive(Serialize, Deserialize)]
pub struct MegaNodeApiUserRunRequest {
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
pub struct MegaNodeApiUserRunResponse {
    pub run_ports: RunPorts,
}

/// A request from a usernode to renew its lease.
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

/// A request to evict a usernode within a meganode.
#[derive(Serialize, Deserialize)]
pub struct MegaNodeApiUserEvictRequest {
    /// The user to be evicted.
    pub user_pk: UserPk,
    /// Sanity check: The meganode to which the request is being sent.
    pub mega_id: MegaId,
}
