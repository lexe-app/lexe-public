use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::Context;
use bitcoin::secp256k1::PublicKey;

use crate::types::{EnclaveId, InstanceId};

/// Derives the instance id from the node public key and enclave measurement.
pub fn get_instance_id(pubkey: &PublicKey, measurement: &str) -> InstanceId {
    let pubkey_hex = pubkey.to_string();

    // TODO(crypto) id derivation scheme;
    // probably hash(pubkey || measurement)
    format!("{}_{}", pubkey_hex, measurement)
}

/// Constructs an enclave id given the instance id and CPU id.
pub fn get_enclave_id(instance_id: &str, cpu_id: &str) -> EnclaveId {
    format!("{}_{}", instance_id, cpu_id)
}

/// Serializes a peer's PublicKey and SocketAddr to <pubkey>@<addr>.
#[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
pub fn peer_pubkey_addr_to_string(
    peer_pubkey: PublicKey,
    peer_address: SocketAddr,
) -> String {
    let pubkey_str = peer_pubkey.to_string();
    let addr_str = peer_address.to_string();
    [pubkey_str, addr_str].join("@")
}

/// Tries to deserialize a peer's PublicKey and SocketAddr from <pubkey>@<addr>.
pub fn peer_pubkey_addr_from_string(
    pubkey_at_addr: String,
) -> anyhow::Result<(PublicKey, SocketAddr)> {
    // vec![<pubkey>, <addr>]
    let mut pubkey_and_addr = pubkey_at_addr.split('@');
    let pubkey_str = pubkey_and_addr
        .next()
        .context("Missing <pubkey> in <pubkey>@<addr> peer address")?;
    let addr_str = pubkey_and_addr
        .next()
        .context("Missing <addr> in <pubkey>@<addr> peer address")?;

    let peer_pubkey = PublicKey::from_str(pubkey_str)
        .context("Could not deserialize PublicKey from LowerHex")?;
    let peer_addr = SocketAddr::from_str(addr_str)
        .context("Could not parse socket address from string")?;

    Ok((peer_pubkey, peer_addr))
}
