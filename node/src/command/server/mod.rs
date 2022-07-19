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

use http::response::Response;
use http::status::StatusCode;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use warp::hyper::Body;
use warp::{reply, Filter, Rejection, Reply};

use crate::command::{host, owner};
use crate::peer_manager::LexePeerManager;
use crate::types::ChannelManagerType;

mod inject;

/// Errors that can be returned to callers of the command API.
#[derive(Error, Debug)]
pub enum ApiError {}

impl Reply for ApiError {
    fn into_response(self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(self.to_string().into())
            .expect("Could not construct Response")
    }
}

/// Converts Result<S, E> into Response<Body>, avoiding the need to call
/// reply::json(&resp) in every handler or to implement warp::Reply manually
fn into_response<S: Serialize, E: Reply>(
    reply_res: Result<S, E>,
) -> Response<Body> {
    match reply_res {
        Ok(resp) => reply::json(&resp).into_response(),
        Err(err) => err.into_response(),
    }
}

/// All routes exposed by the command server.
pub fn routes(
    channel_manager: Arc<ChannelManagerType>,
    peer_manager: Arc<LexePeerManager>,
    activity_tx: mpsc::Sender<()>,
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let root = warp::path::end().map(|| "This is a Lexe user node.");

    let owner = owner(channel_manager, peer_manager, activity_tx);
    let host = host(shutdown_tx);

    // TODO return a 404 not found if no routes were hit
    root.or(host).or(owner)
}

/// Endpoints that can only be called by the node owner.
fn owner(
    channel_manager: Arc<ChannelManagerType>,
    peer_manager: Arc<LexePeerManager>,
    activity_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    // TODO Add owner authentication to this base path
    let owner = warp::path("owner")
        .map(move || {
            // Hitting any endpoint under /owner counts as activity
            println!("Sending activity event");
            let _ = activity_tx.try_send(());
        })
        .untuple_one();

    let node_info = warp::path("node_info")
        .and(warp::get())
        .and(inject::channel_manager(channel_manager))
        .and(inject::peer_manager(peer_manager))
        .then(owner::node_info)
        .map(into_response);

    owner.and(node_info)
}

/// Endpoints that can only be called by the host (Lexe).
fn host(
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    // TODO Add host authentication to this base path
    let host = warp::path("host");

    let status = warp::path("status").and(warp::get()).then(host::status);
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(inject::shutdown_tx(shutdown_tx))
        .then(host::shutdown);

    host.and(status.or(shutdown))
}
