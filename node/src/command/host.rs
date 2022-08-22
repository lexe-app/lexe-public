use common::api::qs::GetByUserPk;
use common::api::UserPk;
use tokio::sync::broadcast;
use warp::Reply;

use crate::command::server::ApiError;

/// GET /host/status -> TODO
pub async fn status(
    query: GetByUserPk,
    user_pk: UserPk,
) -> Result<impl Reply, ApiError> {
    let expected_pk = user_pk;
    let actual_pk = query.user_pk;

    if expected_pk == actual_pk {
        // TODO Actually get status
        Ok("OK")
    } else {
        Err(ApiError::WrongUserPk {
            expected_pk,
            actual_pk,
        })
    }
}

/// GET /host/shutdown -> "Shutdown signal sent"
pub async fn shutdown(
    query: GetByUserPk,
    user_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<impl Reply, ApiError> {
    let expected_pk = user_pk;
    let actual_pk = query.user_pk;

    if expected_pk == actual_pk {
        let _ = shutdown_tx.send(());
        Ok("Shutdown signal sent")
    } else {
        Err(ApiError::WrongUserPk {
            expected_pk,
            actual_pk,
        })
    }
}
