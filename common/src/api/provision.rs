use bitcoin::secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::{MachineId, Measurement, MinCpusvn};

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
