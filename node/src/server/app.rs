use std::{ops::Deref, slice, sync::Arc};

use anyhow::{ensure, Context};
use axum::extract::State;
use common::{
    api::{
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            CreateOfferRequest, CreateOfferResponse, GetAddressResponse,
            GetNewPayments, ListChannelsResponse, NodeInfo, OpenChannelRequest,
            OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
            PayOfferRequest, PayOfferResponse, PayOnchainRequest,
            PayOnchainResponse, PaymentIndexes, PreflightCloseChannelRequest,
            PreflightCloseChannelResponse, PreflightOpenChannelRequest,
            PreflightOpenChannelResponse, PreflightPayInvoiceRequest,
            PreflightPayInvoiceResponse, PreflightPayOfferRequest,
            PreflightPayOfferResponse, PreflightPayOnchainRequest,
            PreflightPayOnchainResponse, UpdatePaymentNote,
        },
        error::NodeApiError,
        models::{
            SignMsgRequest, SignMsgResponse, VerifyMsgRequest,
            VerifyMsgResponse,
        },
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            GetRevocableClients, RevocableClients, UpdateClientRequest,
            UpdateClientResponse,
        },
        Empty,
    },
    constants::{self},
    ln::{amount::Amount, channel::LxUserChannelId, payments::VecBasicPayment},
    rng::SysRng,
    task::MaybeLxTask,
};
use lexe_api::server::{extract::LxQuery, LxJson};
use lexe_ln::{command::CreateInvoiceCaller, p2p};
use tracing::warn;

use super::AppRouterState;

pub(super) async fn node_info(
    State(state): State<Arc<AppRouterState>>,
) -> LxJson<NodeInfo> {
    let channels = state.channel_manager.list_channels();
    LxJson(lexe_ln::command::node_info(
        state.version.clone(),
        state.measurement,
        state.user_pk,
        &state.channel_manager,
        &state.peer_manager,
        &state.wallet,
        &state.chain_monitor,
        &channels,
        state.lsp_info.lsp_fees(),
    ))
}

pub(super) async fn list_channels(
    State(state): State<Arc<AppRouterState>>,
) -> Result<LxJson<ListChannelsResponse>, NodeApiError> {
    let channels = state.channel_manager.list_channels();
    lexe_ln::command::list_channels(
        &state.network_graph,
        &state.chain_monitor,
        channels,
    )
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn sign_message(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<SignMsgRequest>,
) -> LxJson<SignMsgResponse> {
    let sig = state.keys_manager.sign_message(&req.msg);
    LxJson(SignMsgResponse { sig })
}

pub(super) async fn verify_message(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<VerifyMsgRequest>,
) -> LxJson<VerifyMsgResponse> {
    let VerifyMsgRequest { msg, sig, pk } = &req;
    let is_valid = state.keys_manager.verify_message(msg, sig, pk);
    LxJson(VerifyMsgResponse { is_valid })
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

    ensure_channel_value_in_range(&req.value)?;

    let user_channel_id = LxUserChannelId::from_rng(&mut SysRng::new());
    let lsp_node_pk = &lsp_info.node_pk;
    let lsp_addrs = slice::from_ref(&lsp_info.private_p2p_addr);

    // Callback ensures we're connected to the LSP.
    let eph_tasks_tx = state.eph_tasks_tx.clone();
    let ensure_lsp_connected = || async move {
        let maybe_task = p2p::connect_peer_if_necessary(
            peer_manager,
            lsp_node_pk,
            lsp_addrs,
        )
        .await
        .context("Could not connect to Lexe LSP")?;

        if let MaybeLxTask(Some(task)) = maybe_task {
            if eph_tasks_tx.try_send(task).is_err() {
                warn!("(open_channel) Couldn't send task");
            }
        }

        Ok(())
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
        **state.config.load(),
        is_jit_channel,
    )
    .await
    .context("Failed to open channel")
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_open_channel(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightOpenChannelRequest>,
) -> Result<LxJson<PreflightOpenChannelResponse>, NodeApiError> {
    ensure_channel_value_in_range(&req.value)?;

    lexe_ln::command::preflight_open_channel(&state.wallet, req)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

/// Check the `open_channel` value against the min/max bounds early so it fails
/// in preflight with a good error message.
fn ensure_channel_value_in_range(value: &Amount) -> Result<(), NodeApiError> {
    let value = value.sats_u64();

    let min_value = constants::LSP_USERNODE_CHANNEL_MIN_FUNDING_SATS as u64;
    if value < min_value {
        return Err(NodeApiError::command(format!(
            "Channel value is below limit ({min_value} sats)"
        )));
    }

    let max_value = constants::CHANNEL_MAX_FUNDING_SATS as u64;
    if value > max_value {
        return Err(NodeApiError::command(format!(
            "Channel value is above limit ({max_value} sats)"
        )));
    }

    Ok(())
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
    let eph_tasks_tx = state.eph_tasks_tx.clone();
    let ensure_lsp_connected = |node_pk| async move {
        ensure!(&node_pk == lsp_node_pk, "Can only connect to the Lexe LSP");

        let maybe_task = p2p::connect_peer_if_necessary(
            peer_manager,
            lsp_node_pk,
            lsp_addrs,
        )
        .await
        .context("Could not connect to Lexe LSP")?;

        if let MaybeLxTask(Some(task)) = maybe_task {
            if eph_tasks_tx.try_send(task).is_err() {
                warn!("(close_channel) Failed to send connection task");
            }
        }

        Ok(())
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

pub(super) async fn preflight_close_channel(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightCloseChannelRequest>,
) -> Result<LxJson<PreflightCloseChannelResponse>, NodeApiError> {
    lexe_ln::command::preflight_close_channel(
        &state.channel_manager,
        &state.chain_monitor,
        &state.fee_estimates,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn create_invoice(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<CreateInvoiceRequest>,
) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
    let caller = CreateInvoiceCaller::UserNode {
        lsp_info: state.lsp_info.clone(),
        intercept_scids: state.intercept_scids.clone(),
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
        &state.network_graph,
        &state.chain_monitor,
        state.lsp_info.lsp_fees(),
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
        &state.network_graph,
        &state.chain_monitor,
        state.lsp_info.lsp_fees(),
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn create_offer(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<CreateOfferRequest>,
) -> Result<LxJson<CreateOfferResponse>, NodeApiError> {
    lexe_ln::command::create_offer(req, &state.channel_manager)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn pay_offer(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PayOfferRequest>,
) -> Result<LxJson<PayOfferResponse>, NodeApiError> {
    lexe_ln::command::pay_offer(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.chain_monitor,
        &state.network_graph,
        state.lsp_info.lsp_fees(),
        &state.lsp_info.node_pk,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_pay_offer(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightPayOfferRequest>,
) -> Result<LxJson<PreflightPayOfferResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_offer(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.chain_monitor,
        &state.network_graph,
        state.lsp_info.lsp_fees(),
        &state.lsp_info.node_pk,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn pay_onchain(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PayOnchainRequest>,
) -> Result<LxJson<PayOnchainResponse>, NodeApiError> {
    let response = lexe_ln::command::pay_onchain(
        req,
        state.network,
        &state.wallet,
        &state.tx_broadcaster,
        &state.payments_manager,
    )
    .await
    .map_err(NodeApiError::command)?;

    Ok(LxJson(response))
}

pub(super) async fn preflight_pay_onchain(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PreflightPayOnchainRequest>,
) -> Result<LxJson<PreflightPayOnchainResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_onchain(req, &state.wallet, state.network)
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_address(
    State(state): State<Arc<AppRouterState>>,
) -> LxJson<GetAddressResponse> {
    // TODO(max): Upstream an `Address::into_unchecked` to avoid clone
    let addr = state.wallet.get_address().as_unchecked().clone();
    LxJson(GetAddressResponse { addr })
}

pub(super) async fn get_payments_by_indexes(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<PaymentIndexes>,
) -> Result<LxJson<VecBasicPayment>, NodeApiError> {
    let payments = state
        .persister
        .read_payments_by_indexes(req)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(VecBasicPayment { payments }))
}

pub(super) async fn get_new_payments(
    State(state): State<Arc<AppRouterState>>,
    LxQuery(req): LxQuery<GetNewPayments>,
) -> Result<LxJson<VecBasicPayment>, NodeApiError> {
    let payments = state
        .persister
        .read_new_payments(req)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(VecBasicPayment { payments }))
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

pub(super) async fn get_revocable_clients(
    State(state): State<Arc<AppRouterState>>,
    LxQuery(req): LxQuery<GetRevocableClients>,
) -> Result<LxJson<RevocableClients>, NodeApiError> {
    let locked_revocable_clients = state.revocable_clients.read().unwrap();

    let revocable_clients = if req.valid_only {
        let clients = locked_revocable_clients
            .iter_valid()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        RevocableClients { clients }
    } else {
        locked_revocable_clients.clone()
    };

    Ok(LxJson(revocable_clients))
}

pub(super) async fn create_revocable_client(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<CreateRevocableClientRequest>,
) -> Result<LxJson<CreateRevocableClientResponse>, NodeApiError> {
    lexe_ln::command::create_revocable_client(
        &state.persister,
        state.eph_ca_cert_der.deref().clone(),
        &state.rev_ca_cert,
        &state.revocable_clients,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn update_revocable_client(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<UpdateClientRequest>,
) -> Result<LxJson<UpdateClientResponse>, NodeApiError> {
    lexe_ln::command::update_revocable_client(
        &state.persister,
        &state.revocable_clients,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}
