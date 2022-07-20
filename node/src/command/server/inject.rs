//! This module contains a collection of warp `Filter`s which inject items that
//! are required for subsequent handlers.

use std::convert::Infallible;
use std::sync::Arc;

use tokio::sync::broadcast;
use warp::Filter;

use crate::lexe::peer_manager::LexePeerManager;
use crate::types::ChannelManagerType;

/// Injects a shutdown_tx.
pub fn shutdown_tx(
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = (broadcast::Sender<()>,), Error = Infallible> + Clone
{
    warp::any().map(move || shutdown_tx.clone())
}

/// Injects a channel manager.
pub fn channel_manager(
    channel_manager: Arc<ChannelManagerType>,
) -> impl Filter<Extract = (Arc<ChannelManagerType>,), Error = Infallible> + Clone
{
    warp::any().map(move || channel_manager.clone())
}

/// Injects a peer manager.
pub fn peer_manager(
    peer_manager: LexePeerManager,
) -> impl Filter<Extract = (LexePeerManager,), Error = Infallible> + Clone {
    warp::any().map(move || peer_manager.clone())
}
