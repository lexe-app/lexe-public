use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use bytes::Bytes;
use lexe_api::{
    error::NodeApiError,
    models::{
        command::{CreateInvoiceRequest, CreateInvoiceResponse, ResyncRequest},
        nwc::nip47::{NwcRequestPayload, NwcResponsePayload},
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
use tracing::warn;

use crate::server::{self, RouterState};

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
    LxQuery(UserPkStruct { user_pk }): LxQuery<UserPkStruct>,
    LxBytes(req): LxBytes,
) -> Result<LxBytes, NodeApiError> {
    // Reject accidental nostr events for other users.
    //
    // This may happen if the intended usernode is evicted and another takes its
    // place at the same port. Unlike our gateway-proxied TLS-in-TLS scheme,
    // there is not a consistent connection that prevents this from happening.
    if state.user_pk != user_pk {
        return Err(NodeApiError::wrong_user_pk(state.user_pk, user_pk));
    }

    // Deserialize the nostr::Event
    let req_event: nostr::Event = serde_json::from_slice(&req)
        .context("Failed to decode NWC request event")
        .map_err(NodeApiError::command)?;

    // Verify the event integrity, reject old replays and expired events
    let now = TimestampMs::now();
    let verified_event = crate::nwc::verify::nwc_request_event(&req_event, now)
        .context("Invalid NWC request event")
        .map_err(NodeApiError::command)?;

    // Fetch the NWC client info
    let mut nwc_client = state
        .persister
        .read_nwc_client(*verified_event.client_nostr_pk())
        .await
        .map_err(NodeApiError::command)?;

    // Ensure this event is actually for the right client+wallet nostr pk's
    nwc_client
        .verify_nwc_request_event(verified_event)
        .map_err(NodeApiError::command)?;

    // The stored info event may predate a change to the supported methods,
    // so try to re-sign it here if stale.
    let mut rng = SysRng::new();
    if nwc_client
        .refresh_info_event(&mut rng, now)
        .map_err(NodeApiError::command)?
    {
        let db_fields =
            nwc_client.encrypt(&mut rng, state.persister.vfs_master_key());
        if let Err(e) = state.persister.persist_nwc_client(db_fields).await {
            warn!("Failed to persist refreshed NWC info event: {e:#}");
        }
    }

    // Decrypt+deserialize the inner NWC request
    let decrypted_json = nwc_client
        .decrypt_nip44_request(&req_event.content)
        .map_err(NodeApiError::command)?;
    let req_payload: NwcRequestPayload = serde_json::from_str(&decrypted_json)
        .context("Failed to parse NWC request")
        .map_err(NodeApiError::command)?;

    let res = server::nwc::handle_nwc_request(&state, &req_payload).await;

    let resp_payload = match res {
        Ok(value) => NwcResponsePayload {
            result_type: req_payload.method,
            result: Some(value),
            error: None,
        },
        Err(error) => {
            warn!("NWC request failed: {error:?}");
            NwcResponsePayload {
                result_type: req_payload.method,
                result: None,
                error: Some(error),
            }
        }
    };

    let resp_json = serde_json::to_string(&resp_payload)
        .expect("NwcResponsePayload serialization cannot fail");

    let nip44_payload = nwc_client
        .encrypt_nip44_response(&mut rng, &resp_json)
        .map_err(NodeApiError::command)?;

    let resp_event = nwc_client
        .build_response(&mut rng, req_event.id, nip44_payload)
        .map_err(NodeApiError::command)?;

    let resp_event_bytes = serde_json::to_vec(&resp_event)
        .context("Failed to serialize nostr::Event")
        .map_err(NodeApiError::command)?;

    Ok(LxBytes(Bytes::from(resp_event_bytes)))
}
