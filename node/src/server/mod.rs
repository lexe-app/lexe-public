//! The warp server that the node uses to:
//!
//! 1) Accept commands from the app (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Obviously, Lexe cannot spend funds on behalf of the user; Lexe's portion of
//! this endpoint is used purely for maintenance tasks such as monitoring and
//! scheduling.
//!
//! TODO Implement app authentication
//! TODO Implement authentication of Lexe

use std::sync::Arc;

use common::{
    api::{
        command::{
            CreateInvoiceRequest, PayInvoiceRequest, SendOnchainRequest,
        },
        qs::{
            GetByUserPk, GetNewPayments, GetPaymentsByIds, UpdatePaymentNote,
        },
        rest, Scid, UserPk,
    },
    cli::{LspInfo, Network},
    shutdown::ShutdownChannel,
};
use lexe_ln::{
    alias::RouterType, command::CreateInvoiceCaller, esplora::LexeEsplora,
    keys_manager::LexeKeysManager, wallet::LexeWallet,
};
use tokio::sync::mpsc;
use tracing::{span, trace};
use warp::{filters::BoxedFilter, http::Response, hyper::Body, Filter, Reply};

use crate::{
    alias::{ChainMonitorType, NodePaymentsManagerType},
    channel_manager::NodeChannelManager,
    peer_manager::NodePeerManager,
    persister::NodePersister,
};

/// Handlers for commands that can only be initiated by the app.
mod app;
/// Handlers for commands that can only be initiated by the runner (Lexe).
mod runner;

/// Implements [`AppNodeRunApi`] - endpoints only callable by the app.
///
/// [`AppNodeRunApi`]: common::api::def::AppNodeRunApi
pub(crate) fn app_routes(
    parent_span: Option<span::Id>,
    persister: NodePersister,
    chain_monitor: Arc<ChainMonitorType>,
    wallet: LexeWallet,
    esplora: Arc<LexeEsplora>,
    router: Arc<RouterType>,
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    keys_manager: Arc<LexeKeysManager>,
    payments_manager: NodePaymentsManagerType,
    lsp_info: LspInfo,
    scid: Scid,
    network: Network,
    activity_tx: mpsc::Sender<()>,
) -> BoxedFilter<(Response<Body>,)> {
    let app_base = warp::path("app")
        .map(move || {
            // Hitting any endpoint under /app counts as activity
            trace!("Sending activity event");
            let _ = activity_tx.try_send(());
        })
        .untuple_one();

    let node_info = warp::path("node_info")
        .and(warp::get())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::peer_manager(peer_manager))
        .and(inject::wallet(wallet.clone()))
        .and(inject::chain_monitor(chain_monitor))
        .then(lexe_ln::command::node_info)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let create_invoice = warp::path("create_invoice")
        .and(warp::post())
        .and(warp::body::json::<CreateInvoiceRequest>())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::keys_manager(keys_manager))
        .and(inject::payments_manager(payments_manager.clone()))
        .and(inject::create_invoice_caller(
            CreateInvoiceCaller::UserNode { lsp_info, scid },
        ))
        .and(inject::network(network))
        .then(lexe_ln::command::create_invoice)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let pay_invoice = warp::path("pay_invoice")
        .and(warp::post())
        .and(warp::body::json::<PayInvoiceRequest>())
        .and(inject::router(router))
        .and(inject::channel_manager(channel_manager))
        .and(inject::payments_manager(payments_manager.clone()))
        .then(lexe_ln::command::pay_invoice)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let send_onchain = warp::path("send_onchain")
        .and(warp::post())
        .and(warp::body::json::<SendOnchainRequest>())
        .and(inject::wallet(wallet.clone()))
        .and(inject::esplora(esplora))
        .and(inject::payments_manager(payments_manager.clone()))
        .then(lexe_ln::command::send_onchain)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let get_address = warp::path("get_address")
        .and(warp::post())
        .and(inject::wallet(wallet))
        .then(lexe_ln::command::get_address)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);

    let get_payments_by_ids = warp::path("ids")
        .and(warp::post())
        .and(warp::body::json::<GetPaymentsByIds>())
        .and(inject::persister(persister.clone()))
        .then(app::get_payments_by_ids)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let get_new_payments = warp::path("new")
        .and(warp::get())
        .and(warp::query::<GetNewPayments>())
        .and(inject::persister(persister))
        .then(app::get_new_payments)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let update_payment_note = warp::path("note")
        .and(warp::put())
        .and(warp::body::json::<UpdatePaymentNote>())
        .and(inject::payments_manager(payments_manager))
        .then(app::update_payment_note)
        .map(convert::anyhow_to_command_api_result)
        .map(rest::into_response);
    let payments = warp::path("payments").and(
        get_payments_by_ids
            .or(get_new_payments)
            .or(update_payment_note),
    );

    let routes = app_base.and(
        node_info
            .or(create_invoice)
            .or(pay_invoice)
            .or(send_onchain)
            .or(get_address)
            .or(payments)
            .map(Reply::into_response),
    );

    routes.with(rest::trace_requests(parent_span)).boxed()
}

// XXX: Add runner authentication
/// Implements [`RunnerNodeApi`] - endpoints only callable by the runner (Lexe).
///
/// [`RunnerNodeApi`]: common::api::def::RunnerNodeApi
pub(crate) fn runner_routes(
    current_pk: UserPk,
    shutdown: ShutdownChannel,
) -> BoxedFilter<(Response<Body>,)> {
    let status = warp::path("status")
        .and(warp::get())
        .and(warp::query::<GetByUserPk>())
        .and(inject::user_pk(current_pk))
        .then(runner::status)
        .map(rest::into_response);
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(warp::query::<GetByUserPk>())
        .and(inject::user_pk(current_pk))
        .and(inject::shutdown(shutdown))
        .map(runner::shutdown)
        .map(rest::into_response);

    let routes = warp::path("runner")
        .and(status.or(shutdown))
        .map(Reply::into_response);

    routes.boxed()
}

mod convert {
    use common::api::error::{NodeApiError, NodeErrorKind};

    /// Converts `anyhow::Result<T>`s to `Result<T, NodeApiError>`s
    /// with error kind [`NodeErrorKind::Command`].
    pub(super) fn anyhow_to_command_api_result<T>(
        anyhow_res: anyhow::Result<T>,
    ) -> Result<T, NodeApiError> {
        anyhow_res.map_err(|e| NodeApiError {
            kind: NodeErrorKind::Command,
            msg: format!("{e:#}"),
        })
    }
}

/// Warp filters for injecting data needed by subsequent filters
mod inject {
    use std::convert::Infallible;

    use super::*;

    pub(super) fn user_pk(
        user_pk: UserPk,
    ) -> impl Filter<Extract = (UserPk,), Error = Infallible> + Clone {
        warp::any().map(move || user_pk)
    }

    pub(super) fn shutdown(
        shutdown: ShutdownChannel,
    ) -> impl Filter<Extract = (ShutdownChannel,), Error = Infallible> + Clone
    {
        warp::any().map(move || shutdown.clone())
    }

    pub(super) fn persister(
        persister: NodePersister,
    ) -> impl Filter<Extract = (NodePersister,), Error = Infallible> + Clone
    {
        warp::any().map(move || persister.clone())
    }

    pub(super) fn chain_monitor(
        chain_monitor: Arc<ChainMonitorType>,
    ) -> impl Filter<Extract = (Arc<ChainMonitorType>,), Error = Infallible> + Clone
    {
        warp::any().map(move || chain_monitor.clone())
    }

    pub(super) fn wallet(
        wallet: LexeWallet,
    ) -> impl Filter<Extract = (LexeWallet,), Error = Infallible> + Clone {
        warp::any().map(move || wallet.clone())
    }

    pub(super) fn esplora(
        esplora: Arc<LexeEsplora>,
    ) -> impl Filter<Extract = (Arc<LexeEsplora>,), Error = Infallible> + Clone
    {
        warp::any().map(move || esplora.clone())
    }

    pub(super) fn router(
        router: Arc<RouterType>,
    ) -> impl Filter<Extract = (Arc<RouterType>,), Error = Infallible> + Clone
    {
        warp::any().map(move || router.clone())
    }

    pub(super) fn channel_manager(
        channel_manager: NodeChannelManager,
    ) -> impl Filter<Extract = (NodeChannelManager,), Error = Infallible> + Clone
    {
        warp::any().map(move || channel_manager.clone())
    }

    pub(super) fn peer_manager(
        peer_manager: NodePeerManager,
    ) -> impl Filter<Extract = (NodePeerManager,), Error = Infallible> + Clone
    {
        warp::any().map(move || peer_manager.clone())
    }

    pub(super) fn keys_manager(
        keys_manager: Arc<LexeKeysManager>,
    ) -> impl Filter<Extract = (Arc<LexeKeysManager>,), Error = Infallible> + Clone
    {
        warp::any().map(move || keys_manager.clone())
    }

    pub(super) fn payments_manager(
        payments_manager: NodePaymentsManagerType,
    ) -> impl Filter<Extract = (NodePaymentsManagerType,), Error = Infallible> + Clone
    {
        warp::any().map(move || payments_manager.clone())
    }

    pub(super) fn create_invoice_caller(
        create_invoice_caller: CreateInvoiceCaller,
    ) -> impl Filter<Extract = (CreateInvoiceCaller,), Error = Infallible> + Clone
    {
        warp::any().map(move || create_invoice_caller.clone())
    }

    pub(super) fn network(
        network: Network,
    ) -> impl Filter<Extract = (Network,), Error = Infallible> + Clone {
        warp::any().map(move || network)
    }
}
