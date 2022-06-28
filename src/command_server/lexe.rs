use warp::Reply;

use crate::command_server::ApiError;

pub async fn status() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok(String::from("Status"))
}

pub async fn shutdown() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok(String::from("Shut down"))
}
