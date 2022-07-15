use std::convert::TryInto;
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::{anyhow, Context};
use bitcoin::hash_types::Txid;
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::BlockHash;
use lightning::chain::keysinterface::{KeysInterface, KeysManager, Recipient};
use lightning_block_sync::http::JsonResponse;

/// Rederives the node public key from the KeysManager
pub fn derive_pubkey(keys_manager: &KeysManager) -> anyhow::Result<PublicKey> {
    let privkey = keys_manager
        .get_node_secret(Recipient::Node)
        .map_err(|()| anyhow!("Decode error: invalid value"))?;
    let mut secp = Secp256k1::new();
    secp.seeded_randomize(&keys_manager.get_secure_random_bytes());
    let derived_pubkey = PublicKey::from_secret_key(&secp, &privkey);
    Ok(derived_pubkey)
}

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
pub fn get_instance_id(pubkey: &PublicKey, measurement: &str) -> String {
    let pubkey_hex = pubkey_to_hex(pubkey);

    // TODO(crypto) id derivation scheme;
    // probably hash(pubkey || measurement)
    format!("{}_{}", pubkey_hex, measurement)
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

pub struct FundedTx {
    pub changepos: i64,
    pub hex: String,
}

impl TryInto<FundedTx> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<FundedTx> {
        Ok(FundedTx {
            changepos: self.0["changepos"].as_i64().unwrap(),
            hex: self.0["hex"].as_str().unwrap().to_string(),
        })
    }
}

pub struct RawTx(pub String);

impl TryInto<RawTx> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<RawTx> {
        Ok(RawTx(self.0.as_str().unwrap().to_string()))
    }
}

pub struct SignedTx {
    pub complete: bool,
    pub hex: String,
}

impl TryInto<SignedTx> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<SignedTx> {
        Ok(SignedTx {
            hex: self.0["hex"].as_str().unwrap().to_string(),
            complete: self.0["complete"].as_bool().unwrap(),
        })
    }
}

pub struct NewAddress(pub String);
impl TryInto<NewAddress> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<NewAddress> {
        Ok(NewAddress(self.0.as_str().unwrap().to_string()))
    }
}

pub struct FeeResponse {
    pub feerate_sat_per_kw: Option<u32>,
    pub errored: bool,
}

impl TryInto<FeeResponse> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<FeeResponse> {
        let errored = !self.0["errors"].is_null();
        Ok(FeeResponse {
            errored,
            // Bitcoin Core gives us a feerate in BTC/KvB, which we need to
            // convert to satoshis/KW. Thus, we first multiply by 10^8 to get
            // satoshis, then divide by 4 to convert virtual-bytes into weight
            // units.
            feerate_sat_per_kw: self.0["feerate"].as_f64().map(
                |feerate_btc_per_kvbyte| {
                    (feerate_btc_per_kvbyte * 100_000_000.0 / 4.0).round()
                        as u32
                },
            ),
        })
    }
}

pub struct BlockchainInfo {
    pub latest_height: usize,
    pub latest_blockhash: BlockHash,
    pub chain: String,
}

impl TryInto<BlockchainInfo> for JsonResponse {
    type Error = std::io::Error;
    fn try_into(self) -> std::io::Result<BlockchainInfo> {
        Ok(BlockchainInfo {
            latest_height: self.0["blocks"].as_u64().unwrap() as usize,
            latest_blockhash: BlockHash::from_hex(
                self.0["bestblockhash"].as_str().unwrap(),
            )
            .unwrap(),
            chain: self.0["chain"].as_str().unwrap().to_string(),
        })
    }
}
