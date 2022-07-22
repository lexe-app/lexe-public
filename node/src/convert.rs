use bitcoin::secp256k1::PublicKey;

use crate::types::{EnclaveId, InstanceId};

// TODO Refactor this away
/// Derives the instance id from the node public key and enclave measurement.
pub fn get_instance_id(pubkey: &PublicKey, measurement: &str) -> InstanceId {
    let pubkey_hex = pubkey.to_string();

    // TODO(crypto) id derivation scheme;
    // probably hash(pubkey || measurement)
    format!("{}_{}", pubkey_hex, measurement)
}

// TODO Refactor this away
/// Constructs an enclave id given the instance id and CPU id.
pub fn get_enclave_id(instance_id: &str, cpu_id: &str) -> EnclaveId {
    format!("{}_{}", instance_id, cpu_id)
}
