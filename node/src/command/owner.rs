use std::sync::Arc;

use common::api::command::ListChannels;
use common::api::error::NodeApiError;
use common::ln::channel::LxChannelDetails;
use lexe_ln::alias::NetworkGraphType;

use crate::channel_manager::NodeChannelManager;

// TODO(max): This should be moved to lexe_ln::command, duplicated (because it
// is so simple), or removed entirely
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
