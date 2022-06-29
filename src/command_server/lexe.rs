use tokio::sync::broadcast;
use warp::Reply;

use crate::command_server::ApiError;

pub async fn status() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok(String::from("Status"))
}

pub async fn shutdown(
    shutdown_tx: broadcast::Sender<()>,
) -> Result<impl Reply, ApiError> {
    let _ = shutdown_tx.send(());
    Ok(String::from("Shutdown successful"))
}
