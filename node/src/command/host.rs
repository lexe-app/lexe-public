use common::api::error::NodeApiError;
use common::api::qs::GetByUserPk;
use common::api::UserPk;
use tokio::sync::broadcast;

/// GET /host/status -> "OK"
pub async fn status(
    query: GetByUserPk,
    user_pk: UserPk,
) -> Result<String, NodeApiError> {
    let saved_pk = user_pk;
    let given_pk = query.user_pk;

    if saved_pk == given_pk {
        // TODO Actually get status
        Ok(String::from("OK"))
    } else {
        Err(NodeApiError::WrongUserPk { saved_pk, given_pk })
    }
}

/// GET /host/shutdown -> ()
pub fn shutdown(
    query: GetByUserPk,
    user_pk: UserPk,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<(), NodeApiError> {
    let saved_pk = user_pk;
    let given_pk = query.user_pk;

    if saved_pk == given_pk {
        let _ = shutdown_tx.send(());
        Ok(())
    } else {
        Err(NodeApiError::WrongUserPk { saved_pk, given_pk })
    }
}
