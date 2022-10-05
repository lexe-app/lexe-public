use common::api::error::NodeApiError;
use common::api::node::NodeInfo;

use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

pub fn node_info<CM, PM, PS>(
    channel_manager: CM,
    peer_manager: PM,
) -> Result<NodeInfo, NodeApiError>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
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
