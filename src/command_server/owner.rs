use warp::Reply;

use crate::command_server::ApiError;

pub async fn node_info() -> Result<impl Reply, ApiError> {
    // TODO Implement
    Ok(String::from("Node info"))
}
