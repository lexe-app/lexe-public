use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use common::{
    api::{models::Status, test_event::TestEventOp, user::UserPkStruct},
    time::TimestampMs,
};
use lexe_api::{
    def::NodeBackendApi,
    error::NodeApiError,
    models::{
        command::ResyncRequest,
        nwc::{
            DbNwcWallet, NostrPk, NwcRequest, NwcResponse,
            nip47::{NwcRequestPayload, NwcResponsePayload},
        },
    },
    server::{LxJson, extract::LxQuery},
    types::Empty,
};
use lexe_ln::test_event;
use tracing::error;

use crate::server::RouterState;

pub(super) async fn status(
    State(state): State<Arc<RouterState>>,
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
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<ResyncRequest>,
) -> Result<LxJson<Empty>, NodeApiError> {
    lexe_ln::command::resync(req, &state.bdk_resync_tx, &state.ldk_resync_tx)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn test_event(
    State(state): State<Arc<RouterState>>,
    LxJson(op): LxJson<TestEventOp>,
) -> Result<LxJson<Empty>, NodeApiError> {
    test_event::do_op(op, &state.test_event_rx)
        .await
        .map(|()| LxJson(Empty {}))
        .map_err(NodeApiError::command)
}

pub(super) async fn shutdown(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<UserPkStruct>,
) -> Result<LxJson<Empty>, NodeApiError> {
    if state.user_pk == req.user_pk {
        state.shutdown.send();
        Ok(LxJson(Empty {}))
    } else {
        Err(NodeApiError::wrong_user_pk(state.user_pk, req.user_pk))
    }
}

pub(super) async fn nwc_request(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<NwcRequest>,
) -> Result<LxJson<NwcResponse>, NodeApiError> {
    use crate::nwc::NwcClient;
    let nwc_client =
        authenticate_nwc_connection(&state, &req.connection_nostr_pk)
            .await
            .map_err(|err| {
                error!("NWC authentication failed: {err:#}");
                NodeApiError::command(err)
            })?;

    let connection =
        NwcClient::from_db(state.persister.vfs_master_key(), nwc_client)
            .context("Failed to decrypt NWC client data")
            .map_err(NodeApiError::command)?;

    let decrypted_json = connection
        .decrypt_nip44_request(&req.sender_nostr_pk, &req.nip44_payload)
        .map_err(NodeApiError::command)?;

    let request_payload: NwcRequestPayload =
        serde_json::from_str(&decrypted_json)
            .context("Failed to parse NWC request")
            .map_err(NodeApiError::command)?;

    let result = super::nwc::handle_nwc_request(&state, &request_payload).await;

    let response_payload = match result {
        Ok(value) => NwcResponsePayload {
            result_type: request_payload.method,
            result: Some(value),
            error: None,
        },
        Err(error) => NwcResponsePayload {
            result_type: request_payload.method,
            result: None,
            error: Some(error),
        },
    };

    let response_json = serde_json::to_string(&response_payload)
        .context("Failed to serialize NWC response")
        .map_err(NodeApiError::command)?;

    let nip44_payload = connection
        .encrypt_nip44_response(&req.sender_nostr_pk, &response_json)
        .map_err(NodeApiError::command)?;

    Ok(LxJson(NwcResponse { nip44_payload }))
}

/// Authenticate an NWC connection by fetching it from the backend.
async fn authenticate_nwc_connection(
    state: &RouterState,
    connection_pk: &NostrPk,
) -> anyhow::Result<DbNwcWallet> {
    let token =
        state.persister.get_token().await.context(
            "Failed to get auth token for NWC client authentication",
        )?;

    let params = lexe_api::models::nwc::GetNwcWalletsParams {
        wallet_nostr_pk: Some(*connection_pk),
    };

    let vec_nwc_wallet = state
        .persister
        .backend_api()
        .get_nwc_wallets(params, token)
        .await
        .context("Failed to fetch NWC clients for authentication")?;

    let mut wallets = vec_nwc_wallet.nwc_wallets;
    wallets
        .pop()
        .ok_or_else(|| anyhow::anyhow!("Unauthorized: Connection not found"))
}
