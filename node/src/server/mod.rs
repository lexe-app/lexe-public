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

use common::api::command::GetInvoiceRequest;
use common::api::error::{NodeApiError, NodeErrorKind};
use common::api::qs::{GetByUserPk, GetNewPayments, GetPaymentsByIds};
use common::api::rest::{into_response, into_succ_response};
use common::api::{Scid, UserPk};
use common::cli::{LspInfo, Network};
use common::ln::invoice::LxInvoice;
use common::shutdown::ShutdownChannel;
use lexe_ln::alias::{NetworkGraphType, PaymentInfoStorageType};
use lexe_ln::command::GetInvoiceCaller;
use lexe_ln::keys_manager::LexeKeysManager;
use tokio::sync::mpsc;
use tracing::trace;
use warp::{Filter, Rejection, Reply};

use crate::alias::NodePaymentsManagerType;
use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;

/// Handlers for commands that can only be initiated by the app.
mod app;
/// Handlers for commands that can only be initiated by the host (Lexe).
mod host;
/// Warp filters for injecting data needed by subsequent filters
mod inject;

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
    persister: NodePersister,
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    network_graph: Arc<NetworkGraphType>,
    keys_manager: LexeKeysManager,
    payments_manager: NodePaymentsManagerType,
    outbound_payments: PaymentInfoStorageType,
    lsp_info: LspInfo,
    scid: Scid,
    network: Network,
    activity_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the app.");

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
        .map(into_succ_response);
    let list_channels = warp::path("channels")
        .and(warp::get())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::network_graph(network_graph))
        .map(app::list_channels)
        .map(into_response);
    let get_invoice = warp::path("get_invoice")
        .and(warp::post())
        .and(warp::body::json::<GetInvoiceRequest>())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::keys_manager(keys_manager))
        .and(inject::payments_manager(payments_manager))
        .and(inject::get_invoice_caller(GetInvoiceCaller::UserNode {
            lsp_info,
            scid,
        }))
        .and(inject::network(network))
        .then(lexe_ln::command::get_invoice)
        .map(into_command_api_result)
        .map(into_response);
    let send_payment = warp::path("send_payment")
        .and(warp::post())
        .and(warp::body::json::<LxInvoice>())
        .and(inject::channel_manager(channel_manager))
        .and(inject::outbound_payments(outbound_payments))
        .map(lexe_ln::command::send_payment)
        .map(into_command_api_result)
        .map(into_response);

    let get_payments_by_ids = warp::path("ids")
        .and(warp::post())
        .and(warp::body::json::<GetPaymentsByIds>())
        .and(inject::persister(persister.clone()))
        .then(app::get_payments_by_ids)
        .map(into_response);
    let get_new_payments = warp::path("new")
        .and(warp::get())
        .and(warp::query::<GetNewPayments>())
        .and(inject::persister(persister))
        .then(app::get_new_payments)
        .map(into_response);
    let payments =
        warp::path("payments").and(get_payments_by_ids.or(get_new_payments));

    let app = app_base.and(
        node_info
            .or(list_channels)
            .or(get_invoice)
            .or(send_payment)
            .or(payments),
    );

    root.or(app)
}

// XXX: Add host authentication
/// Implements [`HostNodeApi`] - endpoints only callable by the host (Lexe).
///
/// [`HostNodeApi`]: common::api::def::HostNodeApi
pub(crate) fn host_routes(
    current_pk: UserPk,
    shutdown: ShutdownChannel,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the host.");

    let status = warp::path("status")
        .and(warp::get())
        .and(warp::query::<GetByUserPk>())
        .and(inject::user_pk(current_pk))
        .then(host::status)
        .map(into_response);
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(warp::query::<GetByUserPk>())
        .and(inject::user_pk(current_pk))
        .and(inject::shutdown(shutdown))
        .map(host::shutdown)
        .map(into_response);
    let host = warp::path("host").and(status.or(shutdown));

    root.or(host)
}
