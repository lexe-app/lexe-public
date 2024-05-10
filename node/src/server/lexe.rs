use std::sync::Arc;

use axum::extract::State;
use common::{
    api::{
        command::OpenChannelRequest,
        error::NodeApiError,
        qs::GetByUserPk,
        server::{extract::LxQuery, LxJson},
        Empty,
    },
    test_event::TestEventOp,
};
use lexe_ln::test_event;

use crate::server::LexeRouterState;

pub(super) async fn status(
    State(state): State<Arc<LexeRouterState>>,
    LxQuery(req): LxQuery<GetByUserPk>,
) -> Result<LxJson<Empty>, NodeApiError> {
    if state.user_pk == req.user_pk {
        Ok(LxJson(Empty {}))
    } else {
        Err(NodeApiError::wrong_user_pk(state.user_pk, req.user_pk))
    }
}

pub(super) async fn resync(
    State(state): State<Arc<LexeRouterState>>,
) -> Result<LxJson<Empty>, NodeApiError> {
    lexe_ln::command::resync(&state.bdk_resync_tx, &state.ldk_resync_tx)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn open_channel(
    State(state): State<Arc<LexeRouterState>>,
    LxJson(req): LxJson<OpenChannelRequest>,
) -> Result<LxJson<Empty>, NodeApiError> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            use anyhow::Context;
            use common::rng::{RngExt, SysRng};
            use lexe_ln::channel::ChannelRelationship;
            use crate::channel_manager;

            let user_channel_id = SysRng::new().gen_u128();
            let relationship = ChannelRelationship::UserToLsp {
                lsp_channel_peer: state.lsp_info.channel_peer(),
            };
            lexe_ln::channel::open_channel(
                state.channel_manager.clone(),
                state.peer_manager.clone(),
                user_channel_id,
                req.value,
                relationship,
                channel_manager::USER_CONFIG,
            )
            .await
            .map(LxJson)
            .context("Failed to open channel to LSP")
            .map_err(NodeApiError::command)
        } else {
            let _ = state.channel_manager;
            let _ = state.peer_manager;
            let _ = state.lsp_info;
            let _ = req;
            let msg = "This endpoint is disabled in staging/prod";
            Err(NodeApiError::command(msg))
        }
    }
}

pub(super) async fn test_event(
    State(state): State<Arc<LexeRouterState>>,
    LxJson(op): LxJson<TestEventOp>,
) -> Result<LxJson<()>, NodeApiError> {
    test_event::do_op(op, state.test_event_rx.clone())
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn shutdown(
    State(state): State<Arc<LexeRouterState>>,
    LxQuery(req): LxQuery<GetByUserPk>,
) -> Result<LxJson<Empty>, NodeApiError> {
    if state.user_pk == req.user_pk {
        state.shutdown.send();
        Ok(LxJson(Empty {}))
    } else {
        Err(NodeApiError::wrong_user_pk(state.user_pk, req.user_pk))
    }
}
