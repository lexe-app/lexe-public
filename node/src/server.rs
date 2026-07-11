//! The API servers that the node uses to:
//!
//! 1) Accept commands from the user (get balance, send payment etc)
//! 2) Accept housekeeping commands from Lexe (shutdown, health check, etc)
//!
//! Lexe cannot spend funds on behalf of the user; Lexe's endpoints are either
//! used purely for maintenance or only enabled in tests.

use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
};

use axum::{
    Router,
    extract::State,
    routing::{get, post, put},
};
use lexe_api::{
    cli::{LspInfo, OAuthConfig},
    error::NodeApiError,
    models::command::{
        CreateInvoiceRequest, CreateInvoiceResponse, GDriveStatus,
        OnchainDescriptors,
    },
    revocable_clients::RevocableClientsHandle,
    server::LxJson,
    types::{partners::PartnersInfo, payments::OfferId},
};
use lexe_common::{
    api::user::{NodePk, Scid, UserPk},
    env::DeployEnv,
    ln::network::Network,
};
use lexe_enclave::enclave::Measurement;
use lexe_ln::{
    alias::{NetworkGraphType, RouterType},
    channel::ChannelEvent,
    command::CreateInvoiceCaller,
    esplora::FeeEstimates,
    keys_manager::LexeKeysManager,
    sync::BdkSyncRequest,
    test_event::TestEventReceiver,
    tx_broadcaster::TxBroadcaster,
    wallet::OnchainWallet,
};
use lexe_tls::{
    shared_seed::certs::RevocableIssuingCaCert, types::LxCertificateDer,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::util::config::UserConfig;
use tokio::sync::{mpsc, oneshot};
use tower::util::MapRequestLayer;

use crate::{
    alias::{ChainMonitorType, PaymentsManagerType},
    channel_manager::NodeChannelManager,
    peer_manager::NodePeerManager,
    persister::NodePersister,
    runner::UserRunnerCommand,
    user_cache::UserCache,
};

/// Handlers for commands that can only be initiated by the Lexe operators.
mod lexe;
/// Handlers for NWC (Nostr Wallet Connect) commands.
mod nwc;
/// Handlers for commands that can only be initiated by the user.
mod user;

pub(crate) struct RouterState {
    // --- Info --- //
    pub user_pk: UserPk,
    pub network: Network,
    pub measurement: Measurement,
    pub version: semver::Version,
    pub config: Arc<UserConfig>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub lsp_info: LspInfo,
    pub eph_ca_cert_der: Arc<LxCertificateDer>,
    pub rev_ca_cert: Arc<RevocableIssuingCaCert>,
    pub revocable_clients: Arc<RevocableClientsHandle>,
    pub intercept_scids: Vec<Scid>,
    pub gdrive_status: Arc<tokio::sync::Mutex<GDriveStatus>>,
    pub gdrive_oauth_config: Arc<Option<OAuthConfig>>,
    pub deploy_env: DeployEnv,
    pub node_pk: NodePk,
    pub descriptors: OnchainDescriptors,
    pub legacy_descriptors: Option<OnchainDescriptors>,
    pub user_cache: Arc<UserCache>,
    pub partners: Arc<PartnersInfo>,
    pub hba_offer_ids: Arc<RwLock<HashSet<OfferId>>>,

    // --- Actors --- //
    pub channel_manager: NodeChannelManager,
    pub peer_manager: NodePeerManager,
    pub keys_manager: Arc<LexeKeysManager>,
    pub payments_manager: PaymentsManagerType,
    pub network_graph: Arc<NetworkGraphType>,
    pub persister: Arc<NodePersister>,
    pub chain_monitor: Arc<ChainMonitorType>,
    pub router: Arc<RouterType>,
    pub wallet: OnchainWallet,

    // --- Channels --- //
    pub tx_broadcaster: TxBroadcaster,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub runner_tx: mpsc::Sender<UserRunnerCommand>,
    pub bdk_resync_tx: mpsc::Sender<BdkSyncRequest>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub test_event_rx: Arc<tokio::sync::Mutex<TestEventReceiver>>,
    pub shutdown: NotifyOnce,
}

/// Implements [`UserNodeRunApi`] - endpoints only callable by the user.
///
/// [`UserNodeRunApi`]: lexe_api::def::UserNodeRunApi
pub(crate) fn user_router(state: Arc<RouterState>) -> Router<()> {
    let user_pk = state.user_pk;
    let runner_tx = state.runner_tx.clone();

    // Current endpoints, served under both `/user/*` and legacy `/app/*`.
    #[rustfmt::skip]
    let user_routes = Router::new()
        .route("/v2/node_info", get(user::node_info))
        .route("/debug_info", get(user::debug_info))
        .route("/list_channels", get(user::list_channels))
        .route("/sign_message", post(user::sign_message))
        .route("/verify_message", post(user::verify_message))
        .route("/open_channel", post(user::open_channel))
        .route("/preflight_open_channel", post(user::preflight_open_channel))
        .route("/close_channel", post(user::close_channel))
        .route("/preflight_close_channel", post(user::preflight_close_channel))
        .route("/create_invoice", post(shared::create_invoice))
        .route("/pay_invoice", post(user::pay_invoice))
        .route("/preflight_pay_invoice", post(user::preflight_pay_invoice))
        .route("/create_offer", post(user::create_offer))
        .route("/pay_offer", post(user::pay_offer))
        .route("/preflight_pay_offer", post(user::preflight_pay_offer))
        .route("/pay_onchain", post(user::pay_onchain))
        .route("/preflight_pay_onchain", post(user::preflight_pay_onchain))
        .route("/get_address", post(user::get_address))
        .route("/v1/payments/id", get(user::get_payment_by_id))
        .route("/payments/updated", get(user::get_updated_payments))
        .route("/payments/note", put(user::update_personal_note))
        .route("/clients",
            get(user::get_revocable_clients)
                .post(user::create_revocable_client)
                .put(user::update_revocable_client)
        )
        .route("/list_broadcasted_txs", get(user::list_broadcasted_txs))
        .route("/backup", get(user::backup_info))
        .route("/backup/gdrive", post(user::setup_gdrive))
        .route("/v2/human_bitcoin_address",
            get(user::get_human_bitcoin_address)
            .put(user::upsert_custom_human_bitcoin_address)
        )
        .route("/nwc_clients",
            get(user::list_nwc_clients)
                .post(user::create_nwc_client)
                .put(user::update_nwc_client)
                .delete(user::delete_nwc_client)
        );

    // Deprecated endpoints, served under `/app/*` only.
    // Clients never expected these under `/user/*`.
    #[rustfmt::skip]
    let legacy_app_routes = Router::new()
        // TODO(a-mpch): Deprecated since app-v0.8.9+29 and sdk-sidecar-v0.3.1.
        // Remove once unused.
        .route("/payments/indexes", post(user::get_payments_by_indexes))
        .route("/payments/new", get(user::get_new_payments))
        // TODO(a-mpch): Deprecated since app-v0.9.3 and sdk-sidecar-v0.4.2.
        // Remove once unused.
        .route("/payment_address", get(user::get_human_bitcoin_address_v1))
        // TODO(max): Deprecated since app-v0.9.11+49 and sdk-sidecar-v0.4.13.
        // Remove once unused.
        .route("/human_bitcoin_address",
            get(user::get_human_bitcoin_address_v1)
        );

    Router::new()
        .nest("/user", user_routes.clone())
        // compat: Remove once all clients are node-v0.9.12 or later.
        .nest("/app", user_routes.merge(legacy_app_routes))
        .with_state(state)
        // Send an activity notification anytime a user endpoint is hit.
        .layer(MapRequestLayer::new(move |request| {
            let runner_cmd = UserRunnerCommand::UserActivity(user_pk);
            let _ = runner_tx.try_send(runner_cmd);
            request
        }))
}

/// Implements [`LexeNodeRunApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeRunApi`]: lexe_api::def::LexeNodeRunApi
pub(crate) fn lexe_router(state: Arc<RouterState>) -> Router<()> {
    Router::new()
        .route("/lexe/status", get(lexe::status))
        .route("/lexe/resync", post(lexe::resync))
        .route("/lexe/test_event", post(lexe::test_event))
        .route("/lexe/shutdown", get(lexe::shutdown))
        .route("/lexe/create_invoice", post(shared::create_invoice))
        .route("/lexe/nwc_request", post(lexe::nwc_request))
        .with_state(state)
}

mod shared {
    use super::*;

    pub(super) async fn create_invoice(
        State(state): State<Arc<RouterState>>,
        LxJson(req): LxJson<CreateInvoiceRequest>,
    ) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
        let user_exists_fn = state.user_cache.user_exists_fn();
        let caller = CreateInvoiceCaller::UserNode {
            lsp_info: &state.lsp_info,
            intercept_scids: &state.intercept_scids,
            user_exists_fn: &user_exists_fn,
            partners: &state.partners,
        };

        lexe_ln::command::create_invoice(
            req,
            &state.user_pk,
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
}
