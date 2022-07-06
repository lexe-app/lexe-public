use std::sync::Arc;

use serde::Serialize;
use warp::{reply, Reply};

use crate::command_server::ApiError;
use crate::convert;
use crate::types::{ChannelManagerType, PeerManagerType};

#[derive(Serialize)]
pub struct NodeInfo {
    pub pubkey: String,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance_msat: u64,
    pub num_peers: usize,
}

/// GET /owner/node_info -> NodeInfo
pub async fn node_info(
    channel_manager: Arc<ChannelManagerType>,
    peer_manager: Arc<PeerManagerType>,
) -> Result<impl Reply, ApiError> {
    let pubkey = channel_manager.get_our_node_id();
    let pubkey = convert::pubkey_to_hex(&pubkey);

    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let num_usable_channels = channels.iter().filter(|c| c.is_usable).count();

    let local_balance_msat = channels.iter().map(|c| c.balance_msat).sum();
    let num_peers = peer_manager.get_peer_node_ids().len();

    let resp = NodeInfo {
        pubkey,
        num_channels,
        num_usable_channels,
        local_balance_msat,
        num_peers,
    };

    Ok(reply::json(&resp))
}
