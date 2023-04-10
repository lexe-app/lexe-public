//! This module contains a collection of warp `Filter`s which inject items that
//! are required for subsequent handlers.

use std::convert::Infallible;
use std::sync::Arc;

use common::api::UserPk;
use common::cli::Network;
use common::shutdown::ShutdownChannel;
use lexe_ln::alias::{NetworkGraphType, PaymentInfoStorageType, RouterType};
use lexe_ln::command::CreateInvoiceCaller;
use lexe_ln::keys_manager::LexeKeysManager;
use warp::Filter;

use crate::alias::NodePaymentsManagerType;
use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;

/// Injects a [`UserPk`].
pub(crate) fn user_pk(
    user_pk: UserPk,
) -> impl Filter<Extract = (UserPk,), Error = Infallible> + Clone {
    warp::any().map(move || user_pk)
}

/// Injects a [`ShutdownChannel`].
pub(crate) fn shutdown(
    shutdown: ShutdownChannel,
) -> impl Filter<Extract = (ShutdownChannel,), Error = Infallible> + Clone {
    warp::any().map(move || shutdown.clone())
}

/// Injects a [`NodePersister`].
pub(crate) fn persister(
    persister: NodePersister,
) -> impl Filter<Extract = (NodePersister,), Error = Infallible> + Clone {
    warp::any().map(move || persister.clone())
}

/// Injects a router.
pub(crate) fn router(
    router: Arc<RouterType>,
) -> impl Filter<Extract = (Arc<RouterType>,), Error = Infallible> + Clone {
    warp::any().map(move || router.clone())
}

/// Injects a channel manager.
pub(crate) fn channel_manager(
    channel_manager: NodeChannelManager,
) -> impl Filter<Extract = (NodeChannelManager,), Error = Infallible> + Clone {
    warp::any().map(move || channel_manager.clone())
}

/// Injects a peer manager.
pub(crate) fn peer_manager(
    peer_manager: NodePeerManager,
) -> impl Filter<Extract = (NodePeerManager,), Error = Infallible> + Clone {
    warp::any().map(move || peer_manager.clone())
}

/// Injects a network graph.
pub(crate) fn network_graph(
    network_graph: Arc<NetworkGraphType>,
) -> impl Filter<Extract = (Arc<NetworkGraphType>,), Error = Infallible> + Clone
{
    warp::any().map(move || network_graph.clone())
}

/// Injects a keys manager.
pub(crate) fn keys_manager(
    keys_manager: LexeKeysManager,
) -> impl Filter<Extract = (LexeKeysManager,), Error = Infallible> + Clone {
    warp::any().map(move || keys_manager.clone())
}

/// Injects a payments manager.
pub(crate) fn payments_manager(
    payments_manager: NodePaymentsManagerType,
) -> impl Filter<Extract = (NodePaymentsManagerType,), Error = Infallible> + Clone
{
    warp::any().map(move || payments_manager.clone())
}

/// Injects the outbound payments storage.
pub(crate) fn outbound_payments(
    outbound_payments: PaymentInfoStorageType,
) -> impl Filter<Extract = (PaymentInfoStorageType,), Error = Infallible> + Clone
{
    warp::any().map(move || outbound_payments.clone())
}

/// Injects a [`CreateInvoiceCaller`].
pub(crate) fn create_invoice_caller(
    create_invoice_caller: CreateInvoiceCaller,
) -> impl Filter<Extract = (CreateInvoiceCaller,), Error = Infallible> + Clone {
    warp::any().map(move || create_invoice_caller.clone())
}

/// Injects the [`Network`] the node is running on.
pub(crate) fn network(
    network: Network,
) -> impl Filter<Extract = (Network,), Error = Infallible> + Clone {
    warp::any().map(move || network)
}
