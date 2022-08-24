use common::api::error::NodeApiError;
use common::api::qs::GetByUserPk;
use common::api::UserPk;
use tokio::sync::broadcast;

/// GET /host/status -> "OK"
pub async fn status(
    given_pk: GetByUserPk,
    current_pk: UserPk,
) -> Result<String, NodeApiError> {
    if current_pk == given_pk.user_pk {
        // TODO Actually get status
        Ok(String::from("OK"))
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}

/// GET /host/shutdown -> ()
pub fn shutdown(
    given_pk: GetByUserPk,
    current_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<(), NodeApiError> {
    if current_pk == given_pk.user_pk {
        let _ = shutdown_tx.send(());
        Ok(())
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}
