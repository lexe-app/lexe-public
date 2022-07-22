use std::sync::Arc;

use bitcoin::secp256k1::PublicKey;
use serde::Serialize;

use crate::command::server::ApiError;
use crate::lexe::channel_manager::{LexeChannelManager, LxChannelDetails};
use crate::lexe::peer_manager::LexePeerManager;
use crate::types::NetworkGraphType;

#[derive(Serialize)]
pub struct NodeInfo {
    pub pubkey: PublicKey,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance_msat: u64,
    pub num_peers: usize,
}

// TODO Make non-async
/// GET /owner/node_info -> NodeInfo
pub async fn node_info(
    channel_manager: LexeChannelManager,
    peer_manager: LexePeerManager,
) -> Result<NodeInfo, ApiError> {
    let pubkey = channel_manager.get_our_node_id();

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

    Ok(resp)
}

#[derive(Serialize)]
pub struct ListChannels {
    pub channel_details: Vec<LxChannelDetails>,
}

// TODO Make non-async
/// GET /owner/channels -> ListChannels
pub async fn list_channels(
    channel_manager: LexeChannelManager,
    _network_graph: Arc<NetworkGraphType>, // TODO REPL uses it, do we need it?
) -> Result<ListChannels, ApiError> {
    let channel_details = channel_manager
        .list_channels()
        .into_iter()
        .map(LxChannelDetails::from)
        .collect();
    let list_channels = ListChannels { channel_details };
    Ok(list_channels)
}
