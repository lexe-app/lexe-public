use std::sync::Arc;

use common::api::error::NodeApiError;
use common::api::node::{ListChannels, NodeInfo};
use common::ln::channel::LxChannelDetails;

use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::peer_manager::LexePeerManager;
use crate::types::NetworkGraphType;

/// GET /owner/node_info -> NodeInfo
pub fn node_info(
    channel_manager: LexeChannelManager,
    peer_manager: LexePeerManager,
) -> Result<NodeInfo, NodeApiError> {
    let node_pk = channel_manager.get_our_node_id();

    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let num_usable_channels = channels.iter().filter(|c| c.is_usable).count();

    let local_balance_msat = channels.iter().map(|c| c.balance_msat).sum();
    let num_peers = peer_manager.get_peer_node_ids().len();

    let resp = NodeInfo {
        node_pk,
        num_channels,
        num_usable_channels,
        local_balance_msat,
        num_peers,
    };

    Ok(resp)
}

/// GET /owner/channels -> ListChannels
pub fn list_channels(
    channel_manager: LexeChannelManager,
    _network_graph: Arc<NetworkGraphType>, // TODO REPL uses it, do we need it?
) -> Result<ListChannels, NodeApiError> {
    let channel_details = channel_manager
        .list_channels()
        .into_iter()
        .map(LxChannelDetails::from)
        .collect();
    let list_channels = ListChannels { channel_details };
    Ok(list_channels)
}
