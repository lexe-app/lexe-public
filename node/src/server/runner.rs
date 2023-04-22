use common::{
    api::{error::NodeApiError, qs::GetByUserPk, UserPk},
    shutdown::ShutdownChannel,
};

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

pub fn shutdown(
    given: GetByUserPk,
    current_pk: UserPk,
    shutdown: ShutdownChannel,
) -> Result<(), NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        shutdown.send();
        Ok(())
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}
