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

use common::api::command::{CreateInvoiceRequest, PayInvoiceRequest};
use common::api::error::{NodeApiError, NodeErrorKind};
use common::api::qs::{
    GetByUserPk, GetNewPayments, GetPaymentsByIds, UpdatePaymentNote,
};
use common::api::{rest, Scid, UserPk};
use common::cli::{LspInfo, Network};
use common::shutdown::ShutdownChannel;
use lexe_ln::alias::RouterType;
use lexe_ln::command::CreateInvoiceCaller;
use lexe_ln::keys_manager::LexeKeysManager;
use tokio::sync::mpsc;
use tracing::{span, trace};
use warp::filters::BoxedFilter;
use warp::http::Response;
use warp::hyper::Body;
use warp::{Filter, Reply};

use crate::alias::NodePaymentsManagerType;
use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;

/// Handlers for commands that can only be initiated by the app.
mod app;
/// Warp filters for injecting data needed by subsequent filters
mod inject;
/// Handlers for commands that can only be initiated by the runner (Lexe).
mod runner;

/// Converts the `anyhow::Result<T>`s returned by [`lexe_ln::command`] into
/// `Result<T, NodeApiError>`s with error kind [`NodeErrorKind::Command`].
fn into_command_api_result<T>(
    anyhow_res: anyhow::Result<T>,
) -> Result<T, NodeApiError> {
    anyhow_res.map_err(|e| NodeApiError {
        kind: NodeErrorKind::Command,
        msg: format!("{e:#}"),
    })
}

/// Implements [`AppNodeRunApi`] - endpoints only callable by the app.
///
/// [`AppNodeRunApi`]: common::api::def::AppNodeRunApi
pub(crate) fn app_routes(
    parent_span: Option<span::Id>,
    persister: NodePersister,
    router: Arc<RouterType>,
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    keys_manager: LexeKeysManager,
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
        .map(lexe_ln::command::node_info)
        .map(rest::into_succ_response);
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
        .map(into_command_api_result)
        .map(rest::into_response);
    let pay_invoice = warp::path("pay_invoice")
        .and(warp::post())
        .and(warp::body::json::<PayInvoiceRequest>())
        .and(inject::router(router))
        .and(inject::channel_manager(channel_manager))
        .and(inject::payments_manager(payments_manager.clone()))
        .then(lexe_ln::command::pay_invoice)
        .map(into_command_api_result)
        .map(rest::into_response);

    let get_payments_by_ids = warp::path("ids")
        .and(warp::post())
        .and(warp::body::json::<GetPaymentsByIds>())
        .and(inject::persister(persister.clone()))
        .then(app::get_payments_by_ids)
        .map(rest::into_response);
    let get_new_payments = warp::path("new")
        .and(warp::get())
        .and(warp::query::<GetNewPayments>())
        .and(inject::persister(persister))
        .then(app::get_new_payments)
        .map(rest::into_response);
    let update_payment_note = warp::path("note")
        .and(warp::put())
        .and(warp::body::json::<UpdatePaymentNote>())
        .and(inject::payments_manager(payments_manager))
        .then(app::update_payment_note)
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
