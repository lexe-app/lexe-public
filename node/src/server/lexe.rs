use std::sync::Arc;

use anyhow::{Context, anyhow};
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
            DbNwcWallet, GetNwcWalletsParams, NostrPk, NostrSignedEvent,
            NwcRequest,
            nip47::{NwcRequestPayload, NwcResponsePayload},
        },
    },
    server::{LxJson, extract::LxQuery},
    types::Empty,
};
use lexe_ln::test_event;

use crate::{nwc::NwcWallet, server::RouterState};

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
) -> Result<LxJson<NostrSignedEvent>, NodeApiError> {
    let db_nwc_wallet = find_nwc_wallet(&state, &req.wallet_nostr_pk)
        .await
        .map_err(NodeApiError::command)?;

    // This from_db call would fail if the wallet cannot be decrypted, meaning
    // that we either the database is corrupted or the given wallet service
    // public key is invalid or did not correspond to the node.
    let nwc_wallet =
        NwcWallet::from_db(state.persister.vfs_master_key(), db_nwc_wallet)
            .context("Failed to decrypt NWC client data")
            .map_err(NodeApiError::command)?;

    // We validate the client nostr public key here to ensure that the request
    // was sent by the expected client.
    nwc_wallet
        .validate_client_nostr_pk(&req.client_nostr_pk)
        .map_err(NodeApiError::command)?;

    let decrypted_json = nwc_wallet
        .decrypt_nip44_request(&req.nip44_payload)
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

    let nip44_payload = nwc_wallet
        .encrypt_nip44_response(&response_json)
        .map_err(NodeApiError::command)?;

    let response = nwc_wallet
        .build_response(req.event_id, nip44_payload)
        .map_err(NodeApiError::command)?;

    Ok(LxJson(response))
}

/// Find an NWC wallet by the wallet service public key from the request and
/// the user token.
async fn find_nwc_wallet(
    state: &RouterState,
    wallet_nostr_pk: &NostrPk,
) -> anyhow::Result<DbNwcWallet> {
    let token = state
        .persister
        .get_token()
        .await
        .context("Failed to get auth token to fetch NWC wallets")?;

    let params = GetNwcWalletsParams {
        wallet_nostr_pk: Some(*wallet_nostr_pk),
    };

    let vec_nwc_wallet = state
        .persister
        .backend_api()
        .get_nwc_wallets(params, token)
        .await
        .context("Failed to fetch NWC wallets")?;

    let mut wallets = vec_nwc_wallet.nwc_wallets;
    wallets.pop().ok_or_else(|| anyhow!("Wallet not found"))
}
