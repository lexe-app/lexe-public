use common::api::error::NodeApiError;
use common::api::qs::GetByUserPk;
use common::api::UserPk;
use tokio::sync::broadcast;

/// GET /host/status -> "OK"
pub async fn status(
    given: GetByUserPk,
    current_pk: UserPk,
) -> Result<String, NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        // TODO Actually get status
        Ok(String::from("OK"))
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}

/// GET /host/shutdown -> ()
pub fn shutdown(
    given: GetByUserPk,
    current_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<(), NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        let _ = shutdown_tx.send(());
        Ok(())
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}
