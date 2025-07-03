//! The API servers that the node uses to:
//!
//! 1) Accept commands from the app (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Lexe cannot spend funds on behalf of the user; Lexe's endpoints are either
//! used purely for maintenance or only enabled in tests.

use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use arc_swap::ArcSwap;
use axum::{
    routing::{get, post, put},
    Router,
};
use common::{
    api::{
        revocable_clients::RevocableClients,
        user::{Scid, UserPk},
    },
    cli::LspInfo,
    enclave::Measurement,
    ln::network::LxNetwork,
};
use lexe_ln::{
    alias::{NetworkGraphType, RouterType},
    channel::ChannelEvent,
    esplora::FeeEstimates,
    keys_manager::LexeKeysManager,
    sync::BdkSyncRequest,
    test_event::TestEventReceiver,
    tx_broadcaster::TxBroadcaster,
    wallet::LexeWallet,
};
use lexe_tls::{
    shared_seed::certs::RevocableIssuingCaCert, types::LxCertificateDer,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::util::config::UserConfig;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tower::util::MapRequestLayer;

use crate::{
    activity,
    alias::{ChainMonitorType, PaymentsManagerType},
    channel_manager::NodeChannelManager,
    client::RunnerClient,
    peer_manager::NodePeerManager,
    persister::NodePersister,
};

/// Handlers for commands that can only be initiated by the app.
mod app;
/// Handlers for commands that can only be initiated by the Lexe operators.
mod lexe;

pub(crate) struct AppRouterState {
    pub user_pk: UserPk,
    pub network: LxNetwork,
    pub measurement: Measurement,
    pub version: semver::Version,
    pub config: Arc<ArcSwap<UserConfig>>,
    pub runner_api: Arc<RunnerClient>,
    pub persister: Arc<NodePersister>,
    pub chain_monitor: Arc<ChainMonitorType>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub tx_broadcaster: Arc<TxBroadcaster>,
    pub wallet: LexeWallet,
    pub router: Arc<RouterType>,
    pub channel_manager: NodeChannelManager,
    pub peer_manager: NodePeerManager,
    pub keys_manager: Arc<LexeKeysManager>,
    pub payments_manager: PaymentsManagerType,
    pub network_graph: Arc<NetworkGraphType>,
    pub lsp_info: LspInfo,
    pub intercept_scids: Vec<Scid>,
    pub eph_ca_cert_der: Arc<LxCertificateDer>,
    pub rev_ca_cert: Arc<RevocableIssuingCaCert>,
    pub revocable_clients: Arc<RwLock<RevocableClients>>,
    pub user_activity_bus: EventsBus<()>,
    pub mega_activity_bus: EventsBus<UserPk>,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
}

/// Implements [`AppNodeRunApi`] - endpoints only callable by the app.
///
/// [`AppNodeRunApi`]: lexe_api::def::AppNodeRunApi
pub(crate) fn app_router(state: Arc<AppRouterState>) -> Router<()> {
    /// The minimum interval between activity notifications from requests to
    /// `/app` endpoints. Prevents spamming the megarunner with requests.
    const MIN_ACTIVITY_NOTIFY_INTERVAL: Duration = Duration::from_secs(60);

    // The last time we sent a request to runner `/node/activity`.
    let last_activity_callback = Arc::new(Mutex::new(Instant::now()));

    let user_pk = state.user_pk;
    let user_activity_bus = state.user_activity_bus.clone();
    let mega_activity_bus = state.mega_activity_bus.clone();
    let runner_api = state.runner_api.clone();
    let eph_tasks_tx = state.eph_tasks_tx.clone();

    #[rustfmt::skip]
    let router = Router::new()
        .route("/app/node_info", get(app::node_info))
        .route("/app/list_channels", get(app::list_channels))
        .route("/app/sign_message", post(app::sign_message))
        .route("/app/verify_message", post(app::verify_message))
        .route("/app/open_channel", post(app::open_channel))
        .route("/app/preflight_open_channel", post(app::preflight_open_channel))
        .route("/app/close_channel", post(app::close_channel))
        .route("/app/preflight_close_channel", post(app::preflight_close_channel))
        .route("/app/create_invoice", post(app::create_invoice))
        .route("/app/pay_invoice", post(app::pay_invoice))
        .route("/app/preflight_pay_invoice", post(app::preflight_pay_invoice))
        .route("/app/create_offer", post(app::create_offer))
        .route("/app/pay_offer", post(app::pay_offer))
        .route("/app/preflight_pay_offer", post(app::preflight_pay_offer))
        .route("/app/pay_onchain", post(app::pay_onchain))
        .route("/app/preflight_pay_onchain", post(app::preflight_pay_onchain))
        .route("/app/get_address", post(app::get_address))
        .route("/app/payments/indexes", post(app::get_payments_by_indexes))
        .route("/app/payments/new", get(app::get_new_payments))
        .route("/app/payments/note", put(app::update_payment_note))
        .route("/app/clients",
            get(app::get_revocable_clients)
                .post(app::create_revocable_client)
                .put(app::update_revocable_client)
        )
        .with_state(state)
        // Send an activity event and notify the runner anytime /app is hit
        .layer(MapRequestLayer::new(move |request| {
            // Notify the megarunner.
            let mut locked_instant = last_activity_callback.lock().unwrap();
            if locked_instant.elapsed() > MIN_ACTIVITY_NOTIFY_INTERVAL {
                *locked_instant = Instant::now();

                activity::notify_listeners(
                    user_pk,
                    &mega_activity_bus,
                    &user_activity_bus,
                    runner_api.clone(),
                    &eph_tasks_tx,
                );
            }

            request
        }));
    router
}

pub(crate) struct LexeRouterState {
    pub user_pk: UserPk,
    pub bdk_resync_tx: mpsc::Sender<BdkSyncRequest>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub test_event_rx: Arc<tokio::sync::Mutex<TestEventReceiver>>,
    pub shutdown: NotifyOnce,
}

/// Implements [`LexeNodeRunApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeRunApi`]: lexe_api::def::LexeNodeRunApi
pub(crate) fn lexe_router(state: Arc<LexeRouterState>) -> Router<()> {
    Router::new()
        .route("/lexe/status", get(lexe::status))
        .route("/lexe/resync", post(lexe::resync))
        .route("/lexe/test_event", post(lexe::test_event))
        .route("/lexe/shutdown", get(lexe::shutdown))
        .with_state(state)
}
