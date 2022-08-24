use bitcoin::secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::ln::channel::LxChannelDetails;

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub node_pk: PublicKey,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance_msat: u64,
    pub num_peers: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ListChannels {
    pub channel_details: Vec<LxChannelDetails>,
}
