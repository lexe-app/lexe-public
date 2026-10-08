use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use lexe_api::{
    error::NodeApiError,
    models::{
        command::{CreateInvoiceRequest, CreateInvoiceResponse, ResyncRequest},
        nwc::{
            NostrPk,
            nip47::{NwcRequestPayload, NwcResponsePayload},
        },
    },
    server::{LxBytes, LxJson, extract::LxQuery},
    types::Empty,
};
use lexe_common::{
    api::{models::Status, test_event::TestEventOp, user::UserPkStruct},
    time::TimestampMs,
};
use lexe_crypto::rng::SysRng;
use lexe_ln::{background_processor, command::CreateInvoiceCaller, test_event};
use nostr::{Event, util::JsonUtil};
use tracing::warn;

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

pub(super) async fn wait_bgp_quiescent(
    State(state): State<Arc<RouterState>>,
    LxJson(_): LxJson<Empty>,
) -> Result<LxJson<Empty>, NodeApiError> {
    if cfg!(not(any(test, feature = "test-utils"))) {
        return Err(NodeApiError::command(
            "This endpoint is disabled in staging/prod",
        ));
    }
    background_processor::wait_quiescent(&state.bgp_control_tx)
        .await
        .map(|()| LxJson(Empty {}))
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

pub(super) async fn create_invoice(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CreateInvoiceRequest>,
) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
    let user_exists_fn = state.user_cache.user_exists_fn();
    let caller = CreateInvoiceCaller::UserNode {
        lsp_info: &state.lsp_info,
        intercept_scids: &state.intercept_scids,
        user_exists_fn: &user_exists_fn,
        partners: &state.partners,
    };

    let client_pk = None;
    lexe_ln::command::create_invoice(
        req,
        &state.user_pk,
        &state.channel_manager,
        &state.keys_manager,
        &state.payments_manager,
        caller,
        state.network,
        client_pk,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn nwc_request(
    State(state): State<Arc<RouterState>>,
    LxBytes(req): LxBytes,
) -> Result<LxBytes, NodeApiError> {
    let event = Event::from_json(req)
        .context("Failed to decode NWC request event")
        .map_err(NodeApiError::command)?;
    let client_nostr_pk = NostrPk(event.pubkey.to_bytes());
    let nwc_client = state
        .persister
        .read_nwc_client(client_nostr_pk)
        .await
        .map_err(NodeApiError::command)?;

    let wallet_nostr_pk = event
        .tags
        .public_keys()
        .next()
        .map(|pk| NostrPk(pk.to_bytes()))
        .context("Missing wallet #p tag")
        .map_err(NodeApiError::command)?;
    if nwc_client.client_nostr_pk() != &client_nostr_pk {
        return Err(NodeApiError::command("Client nostr pk mismatch"));
    }
    if nwc_client.wallet_nostr_pk() != &wallet_nostr_pk {
        return Err(NodeApiError::command("Wallet nostr pk mismatch"));
    }

    let decrypted_json = nwc_client
        .decrypt_nip44_request(&event.content)
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
        Err(error) => {
            warn!("NWC request failed: {error:?}");
            NwcResponsePayload {
                result_type: request_payload.method,
                result: None,
                error: Some(error),
            }
        }
    };

    let response_json = serde_json::to_string(&response_payload)
        .expect("NwcResponsePayload serialization cannot fail");

    let mut rng = SysRng::new();
    let nip44_payload = nwc_client
        .encrypt_nip44_response(&mut rng, &response_json)
        .map_err(NodeApiError::command)?;

    let response = nwc_client
        .build_response(&mut rng, event.id, nip44_payload)
        .map_err(NodeApiError::command)?;

    Ok(LxBytes::from(response.as_json()))
}
