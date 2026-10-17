//! # Idempotency
//!
//! We don't deduplicate requests, so a method may run more than once for the
//! same request: clients and relays may redeliver an event, clients may retry
//! with a new event after a lost response, and a malicious relay may replay a
//! captured event, even after the node restarts, until it falls outside
//! [`NWC_REQUEST_MAX_CLOCK_SKEW`]. Rather than track seen event ids, which
//! would need a DB write per request to survive restarts, each method must be
//! idempotent or safe to replay:
//!
//! - Read-only methods return current state.
//! - `make_invoice` may create an extra invoice, which just expires unpaid.
//! - Payments must be keyed by payment hash, so a replay finds the existing
//!   payment. A replayed `pay_invoice` should return the original payment's
//!   outcome rather than fail as a duplicate.
//! - Payments without a natural key (e.g. keysend without a client `preimage`)
//!   must derive one from the event id, like `ClientPaymentId`. Event ids are
//!   public, so the derivation must mix in a node secret.
//! - Spending limits must count only new payments, not replays.
//!
//! [`NWC_REQUEST_MAX_CLOCK_SKEW`]: lexe_common::constants::NWC_REQUEST_MAX_CLOCK_SKEW

use anyhow::Context;
use lexe_api::{
    models::{
        command::CreateInvoiceRequest,
        nwc::nip47::{
            GetInfoResult, MakeInvoiceParams, MakeInvoiceResult, NwcError,
            NwcMethod, NwcRequestPayload, NwcTransactionType,
        },
    },
    types::payments::PaymentKind,
};
use lexe_common::ln::{amount::Amount, network::Network};
use lexe_ln::command::CreateInvoiceCaller;
use serde::Deserialize;

use crate::{nwc, server::RouterState};

/// Handle an NWC request by routing to the appropriate command handler.
pub(super) async fn handle_nwc_request(
    state: &RouterState,
    request_payload: &NwcRequestPayload,
) -> Result<serde_json::Value, NwcError> {
    match request_payload.method {
        NwcMethod::GetInfo => {
            let result =
                handle_get_info(state).await.map_err(NwcError::internal)?;
            let value = serde_json::to_value(result)
                .context("Failed to serialize get_info result")
                .map_err(NwcError::internal)?;
            Ok(value)
        }
        NwcMethod::MakeInvoice => {
            let params =
                MakeInvoiceParams::deserialize(&request_payload.params)
                    .context("Invalid make_invoice params")
                    .map_err(NwcError::other)?;
            let result = handle_make_invoice(state, params)
                .await
                .map_err(NwcError::internal)?;
            let value = serde_json::to_value(result)
                .context("Failed to serialize make_invoice result")
                .map_err(NwcError::internal)?;
            Ok(value)
        }
        _ => Err(NwcError::not_implemented("Method not implemented")),
    }
}

async fn handle_get_info(state: &RouterState) -> anyhow::Result<GetInfoResult> {
    let best_block = state.channel_manager.current_best_block();

    // NIP-47 only supports mainnet, regtest, testnet, and signet.
    let network = match state.network {
        Network::Mainnet => "mainnet",
        Network::Regtest => "regtest",
        Network::Testnet4 => "testnet",
        Network::Testnet3 => "testnet",
        Network::Signet => "signet",
    };

    Ok(GetInfoResult {
        alias: format!("lexe-{}", &state.user_pk.to_string()[..8]),
        color: "000000".to_string(),
        pubkey: state.node_pk,
        network: network.to_string(),
        block_height: best_block.height,
        block_hash: best_block.block_hash,
        methods: nwc::SUPPORTED_METHODS.to_vec(),
    })
}

async fn handle_make_invoice(
    state: &RouterState,
    params: MakeInvoiceParams,
) -> anyhow::Result<MakeInvoiceResult> {
    let amount = Amount::from_msat(params.amount_msat);

    let expiry_secs = params.expiry_secs.unwrap_or(3600);

    // TODO(phlip9): NIP-47 allows both `description` and `description_hash`
    // (e.g. zap flows may send both), but `create_invoice` rejects the pair,
    // failing the request as `INTERNAL`. Investigate and fix.
    let create_inv_req = CreateInvoiceRequest {
        expiration_secs: expiry_secs,
        amount: Some(amount),
        description: params.description,
        description_hash: params.description_hash,
        message: None,
        personal_note: None,
        kind: PaymentKind::Invoice,
        partner_pk: None,
        partner_prop_fee: None,
        partner_base_fee: None,
    };

    let user_exists_fn = state.user_cache.user_exists_fn();
    let caller = CreateInvoiceCaller::UserNode {
        lsp_info: &state.lsp_info,
        intercept_scids: &state.intercept_scids,
        user_exists_fn: &user_exists_fn,
        partners: &state.partners,
    };

    // TODO(nicole): Record the NWC client once `PaymentMetadata` has a field
    // for it; `client_pk` is for SDK clients only.
    let client_pk = None;
    let response = lexe_ln::command::create_invoice(
        create_inv_req,
        &state.user_pk,
        &state.channel_manager,
        &state.keys_manager,
        &state.payments_manager,
        caller,
        state.network,
        client_pk,
    )
    .await?;

    let invoice = response.invoice;
    Ok(MakeInvoiceResult {
        transaction_type: NwcTransactionType::Incoming,
        description: invoice.description_str().map(str::to_owned),
        description_hash: invoice.description_hash().copied(),
        payment_hash: invoice.payment_hash(),
        amount_msat: amount.msat(),
        fees_paid_msat: 0,
        created_at_secs: invoice.saturating_created_at().to_secs(),
        expires_at_secs: invoice.saturating_expires_at().to_secs(),
        invoice,
    })
}
