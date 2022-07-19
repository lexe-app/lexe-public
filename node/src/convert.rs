use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::Context;
use bitcoin::hash_types::Txid;
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::secp256k1::PublicKey;

use crate::types::{EnclaveId, InstanceId};

/// Converts a secp PublicKey into a lower hex-encoded String.
///
/// NOTE: Use this function instead of the equivalent in hex.rs
pub fn pubkey_to_hex(pubkey: &PublicKey) -> String {
    format!("{:x}", pubkey)
}

/// Tries to convert a lower hex-encoded String into a secp PublicKey.
///
/// NOTE: Use this function instead of the equivalent in hex.rs
pub fn pubkey_from_hex(pubkey: &str) -> anyhow::Result<PublicKey> {
    PublicKey::from_str(pubkey)
        .context("Could not deserialize PublicKey from LowerHex")
}

/// Derives the instance id from the node public key and enclave measurement.
pub fn get_instance_id(pubkey: &PublicKey, measurement: &str) -> InstanceId {
    let pubkey_hex = pubkey_to_hex(pubkey);

    // TODO(crypto) id derivation scheme;
    // probably hash(pubkey || measurement)
    format!("{}_{}", pubkey_hex, measurement)
}

/// Constructs an enclave id given the instance id and CPU id.
pub fn get_enclave_id(instance_id: &str, cpu_id: &str) -> EnclaveId {
    format!("{}_{}", instance_id, cpu_id)
}

/// Serializes a txid and index into a String of the form <txid>_<index>.
pub fn txid_and_index_to_string(txid: Txid, index: u16) -> String {
    let txid = txid.to_hex();
    let index = index.to_string();

    // <txid>_<index>
    [txid, index].join("_")
}

/// Serializes a peer's PublicKey and SocketAddr to <pubkey>@<addr>.
#[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
pub fn peer_pubkey_addr_to_string(
    peer_pubkey: PublicKey,
    peer_address: SocketAddr,
) -> String {
    let pubkey_str = pubkey_to_hex(&peer_pubkey);
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

/// Attempts to parse a Txid and index from a String of the form <txid>_<index>.
pub fn txid_and_index_from_string(id: String) -> anyhow::Result<(Txid, u16)> {
    let mut txid_and_txindex = id.split('_');
    let txid_str = txid_and_txindex
        .next()
        .context("Missing <txid> in <txid>_<index>")?;
    let index_str = txid_and_txindex
        .next()
        .context("Missing <index> in <txid>_<index>")?;

    let txid =
        Txid::from_hex(txid_str).context("Invalid txid returned from DB")?;
    let index: u16 = index_str
        .to_string()
        .parse()
        .context("Could not parse index into u16")?;

    Ok((txid, index))
}
