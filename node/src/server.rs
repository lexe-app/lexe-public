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
    routing::{get, post},
};
use lexe_api::{
    cli::{LspInfo, OAuthConfig},
    error::NodeApiError,
    models::command::{
        CreateInvoiceRequest, CreateInvoiceResponse, GDriveStatus,
        OnchainDescriptors,
    },
    revocable_clients::{
        ListRevocableClientsHandle, RevocableClientsHandle, scopes::Permission,
    },
    server::{
        LxJson,
        client_authz::{scoped, unscoped},
    },
    types::{partners::PartnersInfo, payments::OfferId},
};
use lexe_common::{
    api::user::{NodePk, Scid, UserPk},
    env::DeployEnv,
    ln::network::Network,
};
use lexe_crypto::hmac;
use lexe_enclave::enclave::Measurement;
use lexe_ln::{
    alias::{NetworkGraphType, RouterType},
    background_processor,
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
    pub continuation_mac_key: hmac::Key,
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
    pub bgp_control_tx: mpsc::Sender<background_processor::QuiescenceRequest>,
    pub tx_broadcaster: TxBroadcaster,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub runner_tx: mpsc::Sender<UserRunnerCommand>,
    pub bdk_resync_tx: mpsc::Sender<BdkSyncRequest>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub test_event_rx: Arc<tokio::sync::Mutex<TestEventReceiver>>,
    pub shutdown: NotifyOnce,
}

impl ListRevocableClientsHandle for RouterState {
    fn list_revocable_clients_handle(&self) -> &RevocableClientsHandle {
        &self.revocable_clients
    }
}

/// Implements [`UserNodeRunApi`] - endpoints only callable by the user.
///
/// [`UserNodeRunApi`]: lexe_api::def::UserNodeRunApi
pub(crate) fn user_router(state: Arc<RouterState>) -> Router<()> {
    let user_pk = state.user_pk;
    let runner_tx = state.runner_tx.clone();

    use Permission::*;

    #[rustfmt::skip]
    let user_routes = Router::new()
        .route("/user/v2/node_info",
            scoped::get(NodeInfo, user::node_info))
        .route("/user/v1/debug_info",
            scoped::get(DebugInfo, user::debug_info))
        .route("/user/v1/list_channels",
            scoped::get(ListChannels, user::list_channels))
        .route("/user/v1/sign_message",
            scoped::post(SignMessage, user::sign_message))
        .route("/user/v1/open_channel",
            scoped::post(OpenChannel, user::open_channel))
        .route("/user/v1/open_channel_preflight",
            scoped::post(OpenChannelPreflight, user::open_channel_preflight))
        .route("/user/v1/close_channel",
            scoped::post(CloseChannel, user::close_channel))
        .route("/user/v1/close_channel_preflight",
            scoped::post(CloseChannelPreflight, user::close_channel_preflight))
        .route("/user/v1/create_invoice",
            scoped::post(CreateInvoice, shared::create_invoice))
        .route("/user/v1/pay_invoice",
            scoped::post(PayInvoice, user::pay_invoice))
        .route("/user/v1/pay_invoice_preflight",
            scoped::post(PayInvoicePreflight, user::pay_invoice_preflight))
        .route("/user/v1/create_offer",
            scoped::post(CreateOffer, user::create_offer))
        .route("/user/v1/pay_offer",
            scoped::post(PayOffer, user::pay_offer))
        .route("/user/v1/pay_offer_preflight",
            scoped::post(PayOfferPreflight, user::pay_offer_preflight))
        .route("/user/v1/create_payer_proof",
            scoped::post(CreatePayerProof, user::create_payer_proof))
        .route("/user/v1/get_next_unused_address",
            scoped::post(GetNextUnusedAddress, user::get_next_unused_address))
        .route("/user/v1/pay_onchain",
            scoped::post(PayOnchain, user::pay_onchain))
        .route("/user/v1/pay_onchain_preflight",
            scoped::post(PayOnchainPreflight, user::pay_onchain_preflight))
        .route("/user/v1/payments/id",
            scoped::get(GetPaymentById, user::get_payment_by_id))
        .route("/user/v1/payments/updated",
            scoped::get(GetUpdatedPayments, user::get_updated_payments))
        .route("/user/v1/payments/note",
            scoped::put(UpdatePersonalNote, user::update_personal_note))
        .route("/user/v1/cancel_payment",
            scoped::post(CancelPayment, user::cancel_payment))
        // Intentionally unscoped: any client may see its own authorization.
        .route("/user/v1/client_info",
            unscoped::get(user::client_info))
        .route("/user/v1/clients",
            scoped::get(ListRevocableClients, user::list_revocable_clients)
                .merge(scoped::post(
                    CreateRevocableClient, user::create_revocable_client,
                ))
                .merge(scoped::put(
                    UpdateRevocableClient, user::update_revocable_client,
                )))
        .route("/user/v1/list_broadcasted_txs",
            scoped::get(ListBroadcastedTxs, user::list_broadcasted_txs))
        .route("/user/v1/backup",
            scoped::get(BackupInfo, user::backup_info))
        .route("/user/v1/backup/gdrive",
            scoped::post(SetupGdrive, user::setup_gdrive))
        .route("/user/v2/human_bitcoin_address",
            scoped::get(GetHumanBitcoinAddress, user::get_human_bitcoin_address)
                .merge(scoped::put(
                    UpdateHumanBitcoinAddress,
                    user::upsert_custom_human_bitcoin_address,
                )))
        .route("/user/v1/nwc_clients",
            scoped::get(ListNwcClients, user::list_nwc_clients)
                .merge(scoped::post(CreateNwcClient, user::create_nwc_client))
                .merge(scoped::put(UpdateNwcClient, user::update_nwc_client))
                .merge(scoped::delete(DeleteNwcClient, user::delete_nwc_client)));

    // Legacy `/app/*` routes for clients predating the migration to `/user`.
    //
    // TODO(max): Remove all `/app` endpoints (this entire router) once all
    // clients are node-v0.9.12 or later
    #[rustfmt::skip]
    let legacy_app_routes = Router::new()
        .route("/app/v2/node_info",
            scoped::get(NodeInfo, user::node_info))
        .route("/app/debug_info",
            scoped::get(DebugInfo, user::debug_info))
        .route("/app/list_channels",
            scoped::get(ListChannels, user::list_channels))
        .route("/app/sign_message",
            scoped::post(SignMessage, user::sign_message))
        .route("/app/open_channel",
            scoped::post(OpenChannel, user::open_channel))
        .route("/app/preflight_open_channel",
            scoped::post(OpenChannelPreflight, user::open_channel_preflight))
        .route("/app/close_channel",
            scoped::post(CloseChannel, user::close_channel))
        .route("/app/preflight_close_channel",
            scoped::post(CloseChannelPreflight, user::close_channel_preflight))
        .route("/app/create_invoice",
            scoped::post(CreateInvoice, shared::create_invoice))
        .route("/app/pay_invoice",
            scoped::post(PayInvoice, user::pay_invoice))
        .route("/app/preflight_pay_invoice",
            scoped::post(PayInvoicePreflight, user::pay_invoice_preflight))
        .route("/app/create_offer",
            scoped::post(CreateOffer, user::create_offer))
        .route("/app/pay_offer",
            scoped::post(PayOffer, user::pay_offer))
        .route("/app/preflight_pay_offer",
            scoped::post(PayOfferPreflight, user::pay_offer_preflight))
        .route("/app/get_address",
            scoped::post(GetNextUnusedAddress, user::get_next_unused_address))
        .route("/app/pay_onchain",
            scoped::post(PayOnchain, user::pay_onchain))
        .route("/app/preflight_pay_onchain",
            scoped::post(PayOnchainPreflight, user::pay_onchain_preflight))
        .route("/app/v1/payments/id",
            scoped::get(GetPaymentById, user::get_payment_by_id))
        // TODO(a-mpch): Deprecated since app-v0.8.9+29 and sdk-sidecar-v0.3.1.
        // Remove once unused.
        .route("/app/payments/indexes",
            scoped::post(GetPaymentsByIndexes, user::get_payments_by_indexes))
        .route("/app/payments/new",
            scoped::get(GetNewPayments, user::get_new_payments))
        .route("/app/payments/updated",
            scoped::get(GetUpdatedPayments, user::get_updated_payments))
        .route("/app/payments/note",
            scoped::put(UpdatePersonalNote, user::update_personal_note))
        .route("/app/clients",
            scoped::get(ListRevocableClients, user::list_revocable_clients)
                .merge(scoped::post(
                    CreateRevocableClient, user::create_revocable_client,
                ))
                .merge(scoped::put(
                    UpdateRevocableClient, user::update_revocable_client,
                )))
        .route("/app/list_broadcasted_txs",
            scoped::get(ListBroadcastedTxs, user::list_broadcasted_txs))
        .route("/app/backup",
            scoped::get(BackupInfo, user::backup_info))
        .route("/app/backup/gdrive",
            scoped::post(SetupGdrive, user::setup_gdrive))
        .route("/app/v2/human_bitcoin_address",
            scoped::get(GetHumanBitcoinAddress, user::get_human_bitcoin_address)
                .merge(scoped::put(
                    UpdateHumanBitcoinAddress,
                    user::upsert_custom_human_bitcoin_address,
                )))
        // TODO(a-mpch): Deprecated since app-v0.9.3 and sdk-sidecar-v0.4.2.
        // Remove once unused.
        .route("/app/payment_address",
            scoped::get(
                GetHumanBitcoinAddress, user::get_human_bitcoin_address_v1,
            ))
        // TODO(max): Deprecated since app-v0.9.11+49 and sdk-sidecar-v0.4.13.
        // Remove once unused.
        .route("/app/human_bitcoin_address",
            scoped::get(
                GetHumanBitcoinAddress, user::get_human_bitcoin_address_v1,
            ))
        .route("/app/nwc_clients",
            scoped::get(ListNwcClients, user::list_nwc_clients)
                .merge(scoped::post(CreateNwcClient, user::create_nwc_client))
                .merge(scoped::put(UpdateNwcClient, user::update_nwc_client))
                .merge(scoped::delete(DeleteNwcClient, user::delete_nwc_client)));

    Router::new()
        .merge(user_routes)
        .merge(legacy_app_routes)
        .with_state(state)
        // Send an activity notification anytime a user endpoint is hit.
        .layer(activity_layer(user_pk, runner_tx))
}

/// Implements [`LexeNodeRunApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeRunApi`]: lexe_api::def::LexeNodeRunApi
pub(crate) fn lexe_router(state: Arc<RouterState>) -> Router<()> {
    // Endpoints which serve user traffic count as user activity;
    // maintenance endpoints must not reset the inactivity timer.
    let substantive_routes = Router::new()
        .route("/lexe/create_invoice", post(shared::create_invoice))
        .route("/lexe/nwc_request", post(lexe::nwc_request))
        .layer(activity_layer(state.user_pk, state.runner_tx.clone()));
    let maintenance_routes = Router::new()
        .route("/lexe/status", get(lexe::status))
        .route("/lexe/resync", post(lexe::resync))
        .route("/lexe/wait_bgp_quiescent", post(lexe::wait_bgp_quiescent))
        .route("/lexe/test_event", post(lexe::test_event))
        .route("/lexe/shutdown", get(lexe::shutdown));

    Router::new()
        .merge(substantive_routes)
        .merge(maintenance_routes)
        .with_state(state)
}

/// A layer which triggers a user activity notification and resets the
/// usernode's inactivity timer every time a request passes through it.
fn activity_layer(
    user_pk: UserPk,
    runner_tx: mpsc::Sender<UserRunnerCommand>,
) -> MapRequestLayer<
    impl Fn(axum::extract::Request) -> axum::extract::Request + Clone,
> {
    MapRequestLayer::new(move |request| {
        let runner_cmd = UserRunnerCommand::UserActivity(user_pk);
        let _ = runner_tx.try_send(runner_cmd);
        request
    })
}

/// Handlers shared by the app (`/app`) and Lexe-operator (`/lexe`) routers.
/// These handlers may differ in the auth wrapper the router applies to them.
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
