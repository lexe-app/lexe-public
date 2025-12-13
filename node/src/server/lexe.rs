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

    // NWC encryption and authentication depend on the set of keys generated
    // on the wallet service side (in our case, the Lexe node).
    //
    // *Key generation*
    //
    // On Client creation, the wallet service generates a client key pair and
    // a wallet service key pair. The client's secret key and the wallet's
    // public key are used in the connection string to establish the
    // communication protocol. The wallet service stores the client's public
    // key and the wallet's public and secret keys in the database.
    //
    // The client's keys are used to sign nostr events, encrypt NWC requests,
    // and decrypt NWC responses. The wallet service keys are used to encrypt
    // NWC responses, validate NWC requests, and sign nostr events.
    //
    // Keys are ephemeral to the connection. Users can drop and re-create new
    // connections at any time.
    //
    // In Lexe's implementation, the client and wallet public keys are stored in
    // plain text in the database, while the wallet service's secret key is
    // stored encrypted in the database as a blob using our implementation
    // of an encryption scheme (see [`common::aes`] for more details).
    //
    // *NWC request*
    //
    // On an NWC request, the node fetches the corresponding client information
    // using the client's public key and decrypts the blob using its own
    // `AesMasterKey` (see [`common::aes`] for more details).
    //
    // Only the node can decrypt the blob, so it is the only one that can
    // retrieve the wallet nostr sk (wallet service secret key in nostr
    // terms).
    //
    // Then, using the NIP-44 nostr encryption protocol, the node decrypts the
    // NWC request payload using the wallet nostr sk and the client nostr
    // pk. The latter is identified by the author of the nostr event
    // (see [`NwcClient::decrypt_nip44_request`]).
    //
    // After decryption, the node can safely use the payload as a node command.
    //
    // *NWC response*
    //
    // After executing the node command, the resulting response is encrypted
    // using the client's public key and the wallet service's secret key.
    //
    // This blob is then only readable by the client that has stored the client
    // nostr sk and has the wallet nostr pk.
    //
    // Then, the node builds the nostr event using the encrypted response blob
    // and the client's nostr pk, and signs it using the wallet nostr sk
    // (see [`NwcClient::build_response`]).
    //
    // The Nostr bridge or the caller of this endpoint can verify the nostr
    // event signature and broadcast the event to the relays.
    //
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
