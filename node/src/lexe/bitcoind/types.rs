use std::convert::TryFrom;
use std::str::FromStr;

use anyhow::anyhow;
use bitcoin::hashes::hex::FromHex;
use bitcoin::BlockHash;
use lightning_block_sync::http::JsonResponse;

use crate::types::Port;

/// The information required to connect to a bitcoind instance via RPC
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitcoindRpcInfo {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: Port,
}

impl BitcoindRpcInfo {
    fn parse_str(s: &str) -> Option<Self> {
        // format: <username>:<password>@<host>:<port>

        let mut parts = s.split(':');
        let (username, pass_host, port) =
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(username), Some(pass_host), Some(port), None) => {
                    (username, pass_host, port)
                }
                _ => return None,
            };

        let mut parts = pass_host.split('@');
        let (password, host) = match (parts.next(), parts.next(), parts.next())
        {
            (Some(password), Some(host), None) => (password, host),
            _ => return None,
        };

        let port = Port::from_str(port).ok()?;

        Some(Self {
            username: username.to_string(),
            password: password.to_string(),
            host: host.to_string(),
            port,
        })
    }
}

impl FromStr for BitcoindRpcInfo {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s).ok_or_else(|| anyhow!("Invalid bitcoind rpc URL"))
    }
}

pub struct FundedTx {
    pub changepos: i64,
    pub hex: String,
}

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for FundedTx {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<FundedTx> {
        Ok(FundedTx {
            changepos: resp.0["changepos"].as_i64().unwrap(),
            hex: resp.0["hex"].as_str().unwrap().to_string(),
        })
    }
}

pub struct RawTx(pub String);

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for RawTx {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<RawTx> {
        Ok(RawTx(resp.0.as_str().unwrap().to_string()))
    }
}

pub struct SignedTx {
    pub complete: bool,
    pub hex: String,
}

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for SignedTx {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<SignedTx> {
        Ok(SignedTx {
            hex: resp.0["hex"].as_str().unwrap().to_string(),
            complete: resp.0["complete"].as_bool().unwrap(),
        })
    }
}

pub struct NewAddress(pub String);

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for NewAddress {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<NewAddress> {
        Ok(NewAddress(resp.0.as_str().unwrap().to_string()))
    }
}

pub struct FeeResponse {
    pub feerate_sat_per_kw: Option<u32>,
    pub errored: bool,
}

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for FeeResponse {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<FeeResponse> {
        let errored = !resp.0["errors"].is_null();
        Ok(FeeResponse {
            errored,
            // Bitcoin Core gives us a feerate in BTC/KvB, which we need to
            // convert to satoshis/KW. Thus, we first multiply by 10^8 to get
            // satoshis, then divide by 4 to convert virtual-bytes into weight
            // units.
            feerate_sat_per_kw: resp.0["feerate"].as_f64().map(
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

// TODO Use serde::Deserialize
impl TryFrom<JsonResponse> for BlockchainInfo {
    type Error = std::io::Error;
    fn try_from(resp: JsonResponse) -> std::io::Result<BlockchainInfo> {
        Ok(BlockchainInfo {
            latest_height: resp.0["blocks"].as_u64().unwrap() as usize,
            latest_blockhash: BlockHash::from_hex(
                resp.0["bestblockhash"].as_str().unwrap(),
            )
            .unwrap(),
            chain: resp.0["chain"].as_str().unwrap().to_string(),
        })
    }
}
