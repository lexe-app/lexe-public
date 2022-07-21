use std::convert::TryFrom;

use bitcoin::hashes::hex::FromHex;
use bitcoin::BlockHash;
use lightning_block_sync::http::JsonResponse;

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
