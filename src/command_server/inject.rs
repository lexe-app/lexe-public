//! This module contains a collection of warp `Filter`s which inject items that
//! are required for subsequent handlers.

use std::convert::Infallible;

use tokio::sync::broadcast;
use warp::Filter;

/// Injects a shutdown_tx.
pub fn shutdown_tx(
    shutdown_tx: broadcast::Sender<()>,
) -> impl Filter<Extract = (broadcast::Sender<()>,), Error = Infallible> + Clone
{
    warp::any().map(move || shutdown_tx.clone())
}
