use bitcoin::secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::{MachineId, Measurement, MinCpusvn};
use crate::root_seed::RootSeed;

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
pub struct ProvisionRequest {
    /// The client's user pk.
    pub user_pk: UserPk,
    /// The client's node public key, derived from the root seed. The node
    /// should sanity check by re-deriving the node pk and checking that it
    /// equals the client's expected value.
    pub node_pk: PublicKey,
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub user_pk: UserPk,
    pub node_pk: PublicKey,
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub node_pk: PublicKey,
    pub measurement: Measurement,
}

/// Uniquely identifies a sealed seed using its primary key fields.
#[derive(Serialize, Deserialize)]
pub struct SealedSeedId {
    pub node_pk: PublicKey,
    pub measurement: Measurement,
    pub machine_id: MachineId,
    pub min_cpusvn: MinCpusvn,
}

#[derive(Serialize, Deserialize)]
pub struct SealedSeed {
    #[serde(flatten)]
    pub id: SealedSeedId,
    pub seed: Vec<u8>,
}

impl SealedSeed {
    pub fn new(
        node_pk: PublicKey,
        measurement: Measurement,
        machine_id: MachineId,
        min_cpusvn: MinCpusvn,
        seed: Vec<u8>,
    ) -> Self {
        Self {
            id: SealedSeedId {
                node_pk,
                measurement,
                machine_id,
                min_cpusvn,
            },
            seed,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceSeed {
    pub node: Node,
    pub instance: Instance,
    pub sealed_seed: SealedSeed,
}
