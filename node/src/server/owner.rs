use std::sync::Arc;

use common::api::command::ListChannels;
use common::api::error::{NodeApiError, NodeErrorKind};
use common::api::qs::GetNewPayments;
use common::ln::channel::LxChannelDetails;
use common::ln::payments::BasicPayment;
use lexe_ln::alias::NetworkGraphType;

use crate::channel_manager::NodeChannelManager;
use crate::persister::NodePersister;

// TODO(max): This should be moved to lexe_ln::command or duplicated (because it
// is so simple) or removed entirely
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

pub(super) async fn get_new_payments(
    req: GetNewPayments,
    persister: NodePersister,
) -> Result<Vec<BasicPayment>, NodeApiError> {
    persister
        .read_new_payments(req)
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Command,
            msg: format!("Could not read `BasicPayment`s: {e:#}"),
        })
}
