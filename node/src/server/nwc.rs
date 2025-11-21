use anyhow::Context;
use bitcoin::hashes::Hash;
use common::ln::{amount::Amount, network::LxNetwork};
use lexe_api::models::{
    command::CreateInvoiceRequest,
    nwc::nip47::{
        GetInfoResult, MakeInvoiceParams, MakeInvoiceResult, NwcError,
        NwcMethod, NwcRequestPayload,
    },
};
use lexe_ln::command::CreateInvoiceCaller;

use crate::server::RouterState;

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
            let params: MakeInvoiceParams =
                serde_json::from_value(request_payload.params.clone())
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

    // Nip47 only supports maintnet, regtest, testnet, and signet.
    let network = match state.network {
        LxNetwork::Mainnet => "mainnet",
        LxNetwork::Regtest => "regtest",
        LxNetwork::Testnet4 => "testnet",
        LxNetwork::Testnet3 => "testnet",
        LxNetwork::Signet => "signet",
    };

    Ok(GetInfoResult {
        alias: format!(
            "lexe-{}",
            state.user_pk.to_string()[..8].to_lowercase()
        ),
        color: "000000".to_string(),
        pubkey: hex::encode(&state.node_pk.serialize()),
        network: network.to_string(),
        block_height: best_block.height,
        block_hash: hex::encode(&best_block.block_hash.to_byte_array()),
        methods: vec!["get_info".to_string(), "make_invoice".to_string()],
    })
}

async fn handle_make_invoice(
    state: &RouterState,
    params: MakeInvoiceParams,
) -> anyhow::Result<MakeInvoiceResult> {
    let amount = Amount::from_msat(params.amount_msat);

    let expiry_secs = params.expiry.unwrap_or(3600);

    let description_hash = if let Some(ref h) = params.description_hash {
        let mut arr = [0u8; 32];
        hex::decode_to_slice(h, &mut arr)
            .context("Invalid description_hash: must be 32 bytes hex")?;
        Some(arr)
    } else {
        None
    };

    let create_inv_req = CreateInvoiceRequest {
        expiry_secs,
        amount: Some(amount),
        description: params.description,
        description_hash,
    };

    let caller = CreateInvoiceCaller::UserNode {
        lsp_info: state.lsp_info.clone(),
        intercept_scids: state.intercept_scids.clone(),
    };

    let response = lexe_ln::command::create_invoice(
        create_inv_req,
        &state.channel_manager,
        &state.keys_manager,
        &state.payments_manager,
        caller,
        state.network,
    )
    .await?;

    let payment_hash = response.invoice.payment_hash();

    Ok(MakeInvoiceResult {
        invoice: response.invoice.to_string(),
        payment_hash: payment_hash.to_string(),
    })
}
