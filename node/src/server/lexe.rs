use std::sync::Arc;

use axum::extract::State;
use common::{
    api::{models::Status, test_event::TestEventOp, user::UserPkStruct},
    time::TimestampMs,
};
use lexe_api::{
    error::NodeApiError,
    models::command::ResyncRequest,
    server::{extract::LxQuery, LxJson},
    types::Empty,
};
use lexe_ln::test_event;

use crate::server::LexeRouterState;

pub(super) async fn status(
    State(state): State<Arc<LexeRouterState>>,
    LxQuery(req): LxQuery<UserPkStruct>,
) -> Result<LxJson<Status>, NodeApiError> {
    if state.user_pk == req.user_pk {
        let timestamp = TimestampMs::now();
        Ok(LxJson(Status { timestamp }))
    } else {
        Err(NodeApiError::wrong_user_pk(state.user_pk, req.user_pk))
    }
}

pub(super) async fn resync(
    State(state): State<Arc<LexeRouterState>>,
    LxJson(req): LxJson<ResyncRequest>,
) -> Result<LxJson<Empty>, NodeApiError> {
    lexe_ln::command::resync(req, &state.bdk_resync_tx, &state.ldk_resync_tx)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn test_event(
    State(state): State<Arc<LexeRouterState>>,
    LxJson(op): LxJson<TestEventOp>,
) -> Result<LxJson<Empty>, NodeApiError> {
    test_event::do_op(op, &state.test_event_rx)
        .await
        .map(|()| LxJson(Empty {}))
        .map_err(NodeApiError::command)
}

pub(super) async fn shutdown(
    State(state): State<Arc<LexeRouterState>>,
    LxQuery(req): LxQuery<UserPkStruct>,
) -> Result<LxJson<Empty>, NodeApiError> {
    if state.user_pk == req.user_pk {
        state.shutdown.send();
        Ok(LxJson(Empty {}))
    } else {
        Err(NodeApiError::wrong_user_pk(state.user_pk, req.user_pk))
    }
}
