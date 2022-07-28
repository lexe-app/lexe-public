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

#[derive(Serialize, Deserialize)]
pub struct SealedSeed {
    pub node_pk: PublicKey,
    pub measurement: Measurement,
    pub machine_id: MachineId,
    pub min_cpusvn: MinCpusvn,
    pub seed: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceSeed {
    pub node: Node,
    pub instance: Instance,
    pub sealed_seed: SealedSeed,
}
