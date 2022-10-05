use std::sync::Arc;

use common::api::error::NodeApiError;
use common::api::node::ListChannels;
use common::ln::channel::LxChannelDetails;
use lexe_ln::alias::NetworkGraphType;

use crate::channel_manager::NodeChannelManager;

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
