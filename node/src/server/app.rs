use std::{slice, sync::Arc};

use anyhow::{ensure, Context};
use axum::extract::State;
use common::{
    api::{
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            ListChannelsResponse, NodeInfo, OpenChannelRequest,
            OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
            PayOnchainRequest, PayOnchainResponse, PreflightPayInvoiceRequest,
            PreflightPayInvoiceResponse, PreflightPayOnchainRequest,
            PreflightPayOnchainResponse,
        },
        error::NodeApiError,
        qs::{GetNewPayments, GetPaymentsByIndexes, UpdatePaymentNote},
        server::{extract::LxQuery, LxJson},
        Empty,
    },
    ln::{channel::LxUserChannelId, payments::BasicPayment},
    rng::SysRng,
};
use lexe_ln::{command::CreateInvoiceCaller, p2p};

use super::AppRouterState;
use crate::channel_manager;

pub(super) async fn node_info(
    State(state): State<Arc<AppRouterState>>,
) -> LxJson<NodeInfo> {
    LxJson(lexe_ln::command::node_info(
        state.version.clone(),
        state.measurement,
        &state.channel_manager,
        &state.peer_manager,
        &state.wallet,
        &state.chain_monitor,
    ))
}

pub(super) async fn list_channels(
    State(state): State<Arc<AppRouterState>>,
) -> LxJson<ListChannelsResponse> {
    LxJson(lexe_ln::command::list_channels(&state.channel_manager))
}

pub(super) async fn open_channel(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<OpenChannelRequest>,
) -> Result<LxJson<OpenChannelResponse>, NodeApiError> {
    let AppRouterState {
        lsp_info,
        peer_manager,
        channel_manager,
        channel_events_bus,
        wallet,
        ..
    } = &*state;

    let user_channel_id = LxUserChannelId::gen(&mut SysRng::new());
    let lsp_node_pk = &lsp_info.node_pk;
    let lsp_addrs = slice::from_ref(&lsp_info.private_p2p_addr);

    // Callback ensures we're connected to the LSP.
    let ensure_lsp_connected = || async move {
        p2p::connect_peer_if_necessary(peer_manager, lsp_node_pk, lsp_addrs)
            .await
            .context("Could not connect to Lexe LSP")
    };

    // Open the channel and wait for `ChannelPending`.
    let is_jit_channel = false;
    lexe_ln::command::open_channel(
        channel_manager,
        channel_events_bus,
        wallet,
        ensure_lsp_connected,
        user_channel_id,
        req.value,
        lsp_node_pk,
        channel_manager::USER_CONFIG,
        is_jit_channel,
    )
    .await
    .context("Failed to open channel")
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn close_channel(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<CloseChannelRequest>,
) -> Result<LxJson<Empty>, NodeApiError> {
    let AppRouterState {
        lsp_info,
        peer_manager,
        channel_manager,
        channel_events_bus,
        ..
    } = &*state;

    // During a cooperative channel close, we want to guarantee that we're
    // connected to the LSP. Proactively reconnect if necessary.
    let lsp_node_pk = &lsp_info.node_pk;
    let lsp_addrs = slice::from_ref(&lsp_info.private_p2p_addr);
    let ensure_lsp_connected = |node_pk| async move {
        ensure!(&node_pk == lsp_node_pk, "Can only connect to the Lexe LSP");
        p2p::connect_peer_if_necessary(peer_manager, lsp_node_pk, lsp_addrs)
            .await
            .context("Could not connect to Lexe LSP")
    };

    lexe_ln::command::close_channel(
        channel_manager,
        channel_events_bus,
        ensure_lsp_connected,
        req,
    )
    .await
    .context("Failed to close channel")
    .map(|()| LxJson(Empty {}))
    .map_err(NodeApiError::command)
}

pub(super) async fn create_invoice(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<CreateInvoiceRequest>,
) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
    let caller = CreateInvoiceCaller::UserNode {
        lsp_info: state.lsp_info.clone(),
        scid: state.scid,
    };
    lexe_ln::command::create_invoice(
        req,
        &state.channel_manager,
        &state.keys_manager,
        &state.payments_manager,
        caller,
        state.network,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn pay_invoice(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PayInvoiceRequest>,
) -> Result<LxJson<PayInvoiceResponse>, NodeApiError> {
    lexe_ln::command::pay_invoice(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_pay_invoice(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightPayInvoiceRequest>,
) -> Result<LxJson<PreflightPayInvoiceResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_invoice(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn pay_onchain(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PayOnchainRequest>,
) -> Result<LxJson<PayOnchainResponse>, NodeApiError> {
    lexe_ln::command::pay_onchain(
        req,
        state.network,
        &state.wallet,
        &state.esplora,
        &state.payments_manager,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_pay_onchain(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightPayOnchainRequest>,
) -> Result<LxJson<PreflightPayOnchainResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_onchain(req, &state.wallet, state.network)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_address(
    State(state): State<Arc<AppRouterState>>,
) -> Result<LxJson<bitcoin::Address>, NodeApiError> {
    lexe_ln::command::get_address(&state.wallet)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_payments_by_indexes(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<GetPaymentsByIndexes>,
) -> Result<LxJson<Vec<BasicPayment>>, NodeApiError> {
    state
        .persister
        .read_payments_by_indexes(req)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_new_payments(
    State(state): State<Arc<AppRouterState>>,
    LxQuery(req): LxQuery<GetNewPayments>,
) -> Result<LxJson<Vec<BasicPayment>>, NodeApiError> {
    state
        .persister
        .read_new_payments(req)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn update_payment_note(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<UpdatePaymentNote>,
) -> Result<LxJson<Empty>, NodeApiError> {
    state
        .payments_manager
        .update_payment_note(req)
        .await
        .map(|()| LxJson(Empty {}))
        .map_err(NodeApiError::command)
}
