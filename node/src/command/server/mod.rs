//! The warp server that the node uses to:
//!
//! 1) Accept commands from its owner (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Obviously, Lexe cannot spend funds on behalf of the user; Lexe's portion of
//! this endpoint is used purely for maintenance tasks such as monitoring and
//! scheduling.
//!
//! TODO Implement owner authentication
//! TODO Implement authentication of Lexe

use std::sync::Arc;

use common::api::rest::into_response;
use common::api::UserPk;
use tokio::sync::{broadcast, mpsc};
use tracing::trace;
use warp::{Filter, Rejection, Reply};

use crate::command::{host, owner};
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::peer_manager::LexePeerManager;
use crate::types::NetworkGraphType;

mod inject;

// TODO Add owner authentication
/// Implements [`OwnerNodeRunApi`] - endpoints only callable by the node owner.
///
/// [`OwnerNodeRunApi`]: common::api::def::OwnerNodeRunApi
pub fn owner_routes(
    channel_manager: LexeChannelManager,
    peer_manager: LexePeerManager,
    network_graph: Arc<NetworkGraphType>,
    activity_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the owner.");

    let owner_base = warp::path("owner")
        .map(move || {
            // Hitting any endpoint under /owner counts as activity
            trace!("Sending activity event");
            let _ = activity_tx.try_send(());
        })
        .untuple_one();

    let node_info = warp::path("node_info")
        .and(warp::get())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::peer_manager(peer_manager))
        .map(owner::node_info)
        .map(into_response);
    let list_channels = warp::path("channels")
        .and(warp::get())
        .and(inject::channel_manager(channel_manager))
        .and(inject::network_graph(network_graph))
        .map(owner::list_channels)
        .map(into_response);

    let owner = owner_base.and(node_info.or(list_channels));

    root.or(owner)
}

// TODO Add host authentication
/// Implements [`HostNodeApi`] - endpoints only callable by the host (Lexe).
///
/// [`HostNodeApi`]: common::api::def::HostNodeApi
pub fn host_routes(
    current_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the host.");

    let status = warp::path("status")
        .and(warp::get())
        .and(warp::query())
        .and(inject::user_pk(current_pk))
        .then(host::status)
        .map(into_response);
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(warp::query())
        .and(inject::user_pk(current_pk))
        .and(inject::shutdown_tx(shutdown_tx))
        .map(host::shutdown)
        .map(into_response);
    let host = warp::path("host").and(status.or(shutdown));

    root.or(host)
}
