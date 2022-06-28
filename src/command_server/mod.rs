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

use http::response::Response;
use http::status::StatusCode;
use thiserror::Error;
use tokio::sync::mpsc;
use warp::hyper::Body;
use warp::{Filter, Rejection, Reply};

mod lexe;
mod owner;

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

// TODO(max): Write a decorater that injects channel manager, peer manager, etc

/// All routes exposed by the command server.
pub fn routes(
    activity_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let root = warp::path::end().map(|| "This is a Lexe user node.");

    let owner = owner(activity_tx);
    let lexe = lexe();

    // TODO return a 404 not found if no routes were hit
    root.or(lexe).or(owner)
}

/// Endpoints that can only be called by the node owner.
fn owner(
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
        // .and(with_db(db.clone()))
        // .and(warp::query())
        .then(owner::node_info);

    owner.and(node_info)
}

/// Endpoints that can only be called by Lexe.
fn lexe() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    // TODO Add Lexe authentication to this base path
    let lexe = warp::path("lexe");

    let status = warp::path("status")
        .and(warp::get())
        // .and(with_db(db.clone()))
        // .and(warp::query())
        .then(lexe::status);
    let shutdown = warp::path("shutdown")
        .and(warp::post())
        // .and(with_db(db.clone()))
        // .and(warp::body::json())
        .then(lexe::shutdown);

    lexe.and(status.or(shutdown))
}
