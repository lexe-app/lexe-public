use std::sync::Arc;

use axum::extract::State;
use common::{
    api::{
        command::{
            CreateInvoiceRequest, CreateInvoiceResponse,
            EstimateFeeSendOnchainResponse, NodeInfo, PayInvoiceRequest,
            PayOnchainRequest, PayOnchainResponse, PreflightPayInvoiceRequest,
            PreflightPayInvoiceResponse, PreflightPayOnchainRequest,
        },
        error::NodeApiError,
        qs::{GetNewPayments, GetPaymentsByIds, UpdatePaymentNote},
        server::{extract::LxQuery, LxJson},
        Empty,
    },
    ln::payments::BasicPayment,
};
use lexe_ln::command::CreateInvoiceCaller;

use super::AppRouterState;

pub(super) async fn node_info(
    State(state): State<Arc<AppRouterState>>,
) -> Result<LxJson<NodeInfo>, NodeApiError> {
    lexe_ln::command::node_info(
        state.version.clone(),
        state.measurement,
        state.channel_manager.clone(),
        state.peer_manager.clone(),
        state.wallet.clone(),
        state.chain_monitor.clone(),
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
        scid: state.scid,
    };
    lexe_ln::command::create_invoice(
        req,
        state.channel_manager.clone(),
        state.keys_manager.clone(),
        state.payments_manager.clone(),
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
) -> Result<LxJson<Empty>, NodeApiError> {
    lexe_ln::command::pay_invoice(
        req,
        state.router.clone(),
        state.channel_manager.clone(),
        state.payments_manager.clone(),
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
        state.router.clone(),
        state.channel_manager.clone(),
        state.payments_manager.clone(),
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
        state.wallet.clone(),
        state.esplora.clone(),
        state.payments_manager.clone(),
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn estimate_fee_send_onchain(
    State(state): State<Arc<AppRouterState>>,
    LxQuery(req): LxQuery<PreflightPayOnchainRequest>,
) -> Result<LxJson<EstimateFeeSendOnchainResponse>, NodeApiError> {
    lexe_ln::command::estimate_fee_send_onchain(req, state.wallet.clone())
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_address(
    State(state): State<Arc<AppRouterState>>,
) -> Result<LxJson<bitcoin::Address>, NodeApiError> {
    lexe_ln::command::get_address(state.wallet.clone())
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_payments_by_ids(
    State(state): State<Arc<AppRouterState>>,
    LxJson(req): LxJson<GetPaymentsByIds>,
) -> Result<LxJson<Vec<BasicPayment>>, NodeApiError> {
    state
        .persister
        .read_payments_by_ids(req)
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
