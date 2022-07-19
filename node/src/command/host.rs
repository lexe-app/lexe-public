use tokio::sync::broadcast;
use warp::Reply;

use crate::command::server::ApiError;

/// GET /host/status -> TODO
pub async fn status() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok("OK")
}

/// GET /host/shutdown -> "Shutdown signal sent"
pub async fn shutdown(
    shutdown_tx: broadcast::Sender<()>,
) -> Result<impl Reply, ApiError> {
    let _ = shutdown_tx.send(());
    Ok("Shutdown signal sent")
}
