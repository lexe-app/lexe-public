use tokio::sync::broadcast;
use warp::Reply;

use crate::command_server::ApiError;

/// GET /lexe/status -> TODO
pub async fn status() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok("OK")
}

/// GET /lexe/shutdown -> "Shutdown signal sent"
pub async fn shutdown(
    shutdown_tx: broadcast::Sender<()>,
) -> Result<impl Reply, ApiError> {
    let _ = shutdown_tx.send(());
    Ok("OK")
}
