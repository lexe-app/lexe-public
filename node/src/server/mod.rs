//! The warp server that the node uses to:
//!
//! 1) Accept commands from its owner (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Obviously, Lexe cannot spend funds on behalf of the user; Lexe's portion of
//! this endpoint is used purely for maintenance tasks such as monitoring and
//! scheduling.
//!
//! TODO Implement owner authentication
//! TODO Implement authentication of Lexe

use std::sync::Arc;

use common::api::command::GetInvoiceRequest;
use common::api::error::{NodeApiError, NodeErrorKind};
use common::api::qs::GetByUserPk;
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

use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;

/// Handlers for commands that can only be initiated by the host (Lexe).
mod host;
/// Warp filters for injecting data needed by subsequent filters
mod inject;
/// Handlers for commands that can only be initiated by the node owner.
mod owner;

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

/// Implements [`OwnerNodeRunApi`] - endpoints only callable by the node owner.
///
/// [`OwnerNodeRunApi`]: common::api::def::OwnerNodeRunApi
pub(crate) fn owner_routes(
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    network_graph: Arc<NetworkGraphType>,
    keys_manager: LexeKeysManager,
    outbound_payments: PaymentInfoStorageType,
    lsp_info: LspInfo,
    scid: Scid,
    network: Network,
    activity_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    let root =
        warp::path::end().map(|| "This set of endpoints is for the owner.");

    let owner_base = warp::path("owner")
        .map(move || {
            // Hitting any endpoint under /owner counts as activity
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
        .map(owner::list_channels)
        .map(into_response);
    let get_invoice = warp::path("get_invoice")
        .and(warp::post())
        .and(inject::channel_manager(channel_manager.clone()))
        .and(inject::keys_manager(keys_manager))
        .and(inject::get_invoice_caller(GetInvoiceCaller::UserNode {
            lsp_info,
            scid,
        }))
        .and(inject::network(network))
        .and(warp::body::json::<GetInvoiceRequest>())
        .map(lexe_ln::command::get_invoice)
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

    let owner = owner_base
        .and(node_info.or(list_channels).or(get_invoice).or(send_payment));

    root.or(owner)
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
