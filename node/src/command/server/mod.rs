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
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use tracing::trace;
use warp::{Filter, Rejection, Reply};

use crate::command::{host, owner};
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::peer_manager::LexePeerManager;
use crate::types::NetworkGraphType;

mod inject;

/// Errors that can be returned to callers of the command API.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ApiError {
    #[error("Wrong user pk; expected '{expected_pk}', received '{actual_pk}'")]
    WrongUserPk {
        expected_pk: UserPk,
        actual_pk: UserPk,
    },
}

// TODO Add owner authentication
/// Endpoints that can only be called by the node owner.
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
/// Endpoints that can only be called by the host (Lexe).
pub fn host_routes(
    user_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the host.");

    let status = warp::path("status")
        .and(warp::get())
        .and(warp::query())
        .and(inject::user_pk(user_pk))
        .then(host::status)
        .map(into_response);
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(warp::query())
        .and(inject::user_pk(user_pk))
        .and(inject::shutdown_tx(shutdown_tx))
        .map(host::shutdown)
        .map(into_response);
    let host = warp::path("host").and(status.or(shutdown));

    root.or(host)
}
