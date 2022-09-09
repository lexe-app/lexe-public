//! This module contains a collection of warp `Filter`s which inject items that
//! are required for subsequent handlers.

use std::convert::Infallible;
use std::sync::Arc;

use common::api::UserPk;
use common::shutdown::ShutdownChannel;
use warp::Filter;

use crate::lexe::channel_manager::NodeChannelManager;
use crate::lexe::peer_manager::NodePeerManager;
use crate::types::NetworkGraphType;

/// Injects a [`UserPk`].
pub fn user_pk(
    user_pk: UserPk,
) -> impl Filter<Extract = (UserPk,), Error = Infallible> + Clone {
    warp::any().map(move || user_pk)
}

/// Injects a [`ShutdownChannel`].
pub fn shutdown(
    shutdown: ShutdownChannel,
) -> impl Filter<Extract = (ShutdownChannel,), Error = Infallible> + Clone {
    warp::any().map(move || shutdown.clone())
}

/// Injects a channel manager.
pub fn channel_manager(
    channel_manager: NodeChannelManager,
) -> impl Filter<Extract = (NodeChannelManager,), Error = Infallible> + Clone {
    warp::any().map(move || channel_manager.clone())
}

/// Injects a peer manager.
pub fn peer_manager(
    peer_manager: NodePeerManager,
) -> impl Filter<Extract = (NodePeerManager,), Error = Infallible> + Clone {
    warp::any().map(move || peer_manager.clone())
}

/// Injects a network graph.
pub fn network_graph(
    network_graph: Arc<NetworkGraphType>,
) -> impl Filter<Extract = (Arc<NetworkGraphType>,), Error = Infallible> + Clone
{
    warp::any().map(move || network_graph.clone())
}
