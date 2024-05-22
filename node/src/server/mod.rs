//! The API servers that the node uses to:
//!
//! 1) Accept commands from the app (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Lexe cannot spend funds on behalf of the user; Lexe's endpoints are either
//! used purely for maintenance or only enabled in tests.

use std::sync::Arc;

use axum::{
    routing::{get, post, put},
    Router,
};
use common::{
    api::{Scid, UserPk},
    cli::{LspInfo, Network},
    enclave::Measurement,
    shutdown::ShutdownChannel,
};
use lexe_ln::{
    alias::RouterType, esplora::LexeEsplora, keys_manager::LexeKeysManager,
    test_event::TestEventReceiver, wallet::LexeWallet,
};
use tokio::sync::{mpsc, oneshot};
use tower::util::MapRequestLayer;
use tracing::debug;

use crate::{
    alias::{ChainMonitorType, NodePaymentsManagerType},
    channel_manager::NodeChannelManager,
    peer_manager::NodePeerManager,
    persister::NodePersister,
};

/// Handlers for commands that can only be initiated by the app.
mod app;
/// Handlers for commands that can only be initiated by the Lexe operators.
mod lexe;

pub(crate) struct AppRouterState {
    pub version: semver::Version,
    pub persister: Arc<NodePersister>,
    pub chain_monitor: Arc<ChainMonitorType>,
    pub wallet: LexeWallet,
    pub esplora: Arc<LexeEsplora>,
    pub router: Arc<RouterType>,
    pub channel_manager: NodeChannelManager,
    pub peer_manager: NodePeerManager,
    pub keys_manager: Arc<LexeKeysManager>,
    pub payments_manager: NodePaymentsManagerType,
    pub lsp_info: LspInfo,
    pub scid: Scid,
    pub network: Network,
    pub measurement: Measurement,
    pub activity_tx: mpsc::Sender<()>,
}

/// Implements [`AppNodeRunApi`] - endpoints only callable by the app.
///
/// [`AppNodeRunApi`]: common::api::def::AppNodeRunApi
pub(crate) fn app_router(state: Arc<AppRouterState>) -> Router<()> {
    let activity_tx = state.activity_tx.clone();
    #[rustfmt::skip]
    let router = Router::new()
        .route("/app/node_info", get(app::node_info))
        .route("/app/create_invoice", post(app::create_invoice))
        .route("/app/pay_invoice", post(app::pay_invoice))
        .route("/app/preflight_pay_invoice", post(app::preflight_pay_invoice))
        .route("/app/pay_onchain", post(app::pay_onchain))
        .route("/app/preflight_pay_onchain", get(app::preflight_pay_onchain))
        .route("/app/get_address", post(app::get_address))
        .route("/app/payments/ids", post(app::get_payments_by_ids))
        .route("/app/payments/new", get(app::get_new_payments))
        .route("/app/payments/note", put(app::update_payment_note))
        .with_state(state)
        // Send an activity event anytime an /app endpoint is hit
        .layer(MapRequestLayer::new(move |request| {
            debug!("Sending activity event");
            let _ = activity_tx.try_send(());
            request
        }));
    router
}

pub(crate) struct LexeRouterState {
    pub user_pk: UserPk,
    pub channel_manager: NodeChannelManager,
    pub peer_manager: NodePeerManager,
    pub lsp_info: LspInfo,
    pub bdk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub test_event_rx: Arc<tokio::sync::Mutex<TestEventReceiver>>,
    pub shutdown: ShutdownChannel,
}

/// Implements [`LexeNodeRunApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeRunApi`]: common::api::def::LexeNodeRunApi
pub(crate) fn lexe_router(state: Arc<LexeRouterState>) -> Router<()> {
    Router::new()
        .route("/lexe/status", get(lexe::status))
        .route("/lexe/resync", post(lexe::resync))
        .route("/lexe/open_channel", post(lexe::open_channel))
        .route("/lexe/test_event", post(lexe::test_event))
        .route("/lexe/shutdown", get(lexe::shutdown))
        .with_state(state)
}
