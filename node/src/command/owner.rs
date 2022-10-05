use std::sync::Arc;

use common::api::error::NodeApiError;
use common::api::node::{ListChannels, NodeInfo};
use common::ln::channel::LxChannelDetails;
use lexe_ln::alias::NetworkGraphType;

use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;

pub fn node_info(
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
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

pub(crate) fn list_channels(
    channel_manager: NodeChannelManager,
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
