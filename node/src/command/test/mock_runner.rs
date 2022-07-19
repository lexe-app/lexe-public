use serde::{Deserialize, Serialize};
use warp::{reply, Filter, Rejection, Reply};

use crate::types::{Port, UserId};

#[derive(Serialize, Deserialize)]
struct UserPort {
    user_id: UserId,
    port: Port,
}

/// Mimics the routes offered to the node by the Runner during init
pub fn routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone
{
    warp::path("ready")
        .and(warp::post())
        .and(warp::body::json())
        .map(|req: UserPort| reply::json(&req))
}
