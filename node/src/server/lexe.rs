use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use common::{
    api::{models::Status, test_event::TestEventOp, user::UserPkStruct},
    rng::SysRng,
    time::TimestampMs,
};
use lexe_api::{
    error::NodeApiError,
    models::{
        command::ResyncRequest,
        nwc::{
            NostrSignedEvent, NwcRequest,
            nip47::{NwcRequestPayload, NwcResponsePayload},
        },
    },
    server::{LxJson, extract::LxQuery},
    types::Empty,
};
use lexe_ln::test_event;

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
) -> Result<LxJson<NostrSignedEvent>, NodeApiError> {
    let nwc_client = state
        .persister
        .read_nwc_client(req.client_nostr_pk)
        .await
        .map_err(NodeApiError::command)?;

    // Check that the client and wallet pks in the request match what we stored.
    if nwc_client.client_nostr_pk() != &req.client_nostr_pk {
        return Err(NodeApiError::command("Client nostr pk mismatch"));
    }
    if nwc_client.wallet_nostr_pk() != &req.wallet_nostr_pk {
        return Err(NodeApiError::command("Wallet nostr pk mismatch"));
    }

    // NIP-44 provides authenticated encryption using ECDH (X25519) + ChaCha20-
    // Poly1305. This guarantees integrity: only someone with the client's
    // secret key could have encrypted the request, and only we (with the
    // wallet's secret key from VFS) can decrypt it. A spoofed client pk in the
    // request would fail decryption since the ECDH shared secret wouldn't
    // match.
    let decrypted_json = nwc_client
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

    let mut rng = SysRng::new();

    let nip44_payload = nwc_client
        .encrypt_nip44_response(&mut rng, &response_json)
        .map_err(NodeApiError::command)?;

    let response = nwc_client
        .build_response(&mut rng, req.event_id, nip44_payload)
        .map_err(NodeApiError::command)?;

    Ok(LxJson(response))
}
