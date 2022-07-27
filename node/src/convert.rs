use bitcoin::secp256k1::PublicKey;
use common::enclave::{MachineId, Measurement};

use crate::types::{EnclaveId, InstanceId};

// TODO Refactor this away
/// Derives the instance id from the node public key and enclave measurement.
pub fn get_instance_id(
    pk: &PublicKey,
    measurement: &Measurement,
) -> InstanceId {
    // TODO(crypto) id derivation scheme;
    // probably hash(pk || measurement)
    format!("{}_{}", pk, measurement)
}

// TODO Refactor this away
/// Constructs an enclave id given the instance id and machine id.
pub fn get_enclave_id(instance_id: &str, machine_id: MachineId) -> EnclaveId {
    format!("{}_{}", instance_id, machine_id)
}
