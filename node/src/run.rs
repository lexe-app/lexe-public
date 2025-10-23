use std::{
    net::TcpListener,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail, ensure};
use bitcoin::{consensus::Encodable, secp256k1};
use common::{
    api::{
        models::BroadcastedTx,
        revocable_clients::RevocableClients,
        user::{GetNewScidsRequest, NodePk, User, UserPk},
    },
    cli::LspInfo,
    constants::{self},
    ed25519,
    enclave::{MachineId, Measurement},
    env::DeployEnv,
    ln::{
        balance::OnchainBalance, channel::LxOutPoint, hashes::LxTxid,
        network::LxNetwork,
    },
    net,
    rng::{Crng, SysRng},
    root_seed::RootSeed,
    time::TimestampMs,
};
use futures::future::FutureExt;
use gdrive::gvfs::GvfsRootName;
use lexe_api::{
    auth::BearerAuthenticator,
    def::{NodeBackendApi, NodeLspApi, NodeRunnerApi},
    error::MegaApiError,
    models::{command::GDriveStatus, runner::UserLeaseRenewalRequest},
    server::LayerConfig,
    types::{ports::RunPorts, sealed_seed::SealedSeedId},
    vfs::{self, REVOCABLE_CLIENTS_FILE_ID, Vfs, VfsFileId},
};
use lexe_ln::{
    BoxedAnyhowFuture,
    alias::{
        BroadcasterType, EsploraSyncClientType, FeeEstimatorType,
        LexeOnionMessengerType, NetworkGraphType, ProbabilisticScorerType,
    },
    background_processor,
    channel_monitor::ChannelMonitorPersister,
    esplora::LexeEsplora,
    event,
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    message_router::LexeMessageRouter,
    payments::manager::PaymentsManager,
    route::LexeRouter,
    sync::{self, BdkSyncRequest},
    test_event,
    traits::LexeInnerPersister,
    tx_broadcaster::TxBroadcaster,
    wallet::{self, LexeCoinSelector, LexeWallet},
};
use lexe_std::{Apply, const_assert};
use lexe_tls::shared_seed::certs::{
    EphemeralIssuingCaCert, RevocableIssuingCaCert,
};
use lexe_tokio::{
    DEFAULT_CHANNEL_SIZE, SMALLER_CHANNEL_SIZE,
    events_bus::EventsBus,
    notify,
    notify_once::NotifyOnce,
    task::{self, LxTask, MaybeLxTask},
};
use lightning::{
    chain::{Watch, chainmonitor::ChainMonitor},
    ln::{peer_handler::IgnoringMessageHandler, types::ChannelId},
};
use lightning_transaction_sync::EsploraSyncClient;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, info_span, warn};

use crate::{
    SEMVER_VERSION,
    alias::{ChainMonitorType, OnionMessengerType, PaymentsManagerType},
    channel_manager::NodeChannelManager,
    client::{NodeBackendClient, RunnerClient},
    context::{MegaContext, UserContext},
    event_handler::{self, NodeEventHandler},
    gdrive_persister,
    gdrive_provision::{self, GoogleVfsInitError},
    p2p,
    peer_manager::NodePeerManager,
    persister::{self, NodePersister},
    server::{self, RouterState},
};

/// The minimum # of intercept scids we want (for inserting into invoices).
///
/// See NOTE above [`lexe_ln::command::MAX_INTERCEPT_HINTS`] for why this is 1.
const MIN_INTERCEPT_SCIDS: usize = 1;
// Ensure we don't request more than we'll ever use.
const_assert!(MIN_INTERCEPT_SCIDS <= lexe_ln::command::MAX_INTERCEPT_HINTS);

/// Run a user node
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunArgs {
    /// protocol://host:port of the backend.
    pub backend_url: String,

    /// Maximum duration for user node leases (in seconds).
    pub lease_lifetime_secs: u64,

    /// Interval at which user nodes should renew their leases (in seconds).
    pub lease_renewal_interval_secs: u64,

    /// info relating to Lexe's LSP.
    pub lsp: LspInfo,

    /// protocol://host:port of the runner.
    pub runner_url: String,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. Can be used to start nodes for maintenance purposes.
    pub shutdown_after_sync: bool,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// Esplora urls which someone in Lexe's infra says we should use.
    /// We'll only use urls contained in our whitelist.
    pub untrusted_esplora_urls: Vec<String>,

    /// bitcoin, testnet, regtest, or signet.
    pub untrusted_network: LxNetwork,

    /// How long the usernode can remain inactive (in seconds) before it gets
    /// evicted by the UserRunner.
    pub user_inactivity_secs: u64,

    /// the Lexe user pk used in queries to the persistence API
    pub user_pk: UserPk,
}

/// A user's node.
#[allow(dead_code)] // Many unread fields are used as type annotations
pub struct UserNode {
    // --- General --- //
    args: RunArgs,
    deploy_env: DeployEnv,
    run_ports: RunPorts,
    static_tasks: Vec<LxTask<()>>,
    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    shutdown: NotifyOnce,
    user_pk: UserPk,
    runner_api: Arc<RunnerClient>,

    // --- Actors --- //
    chain_monitor: Arc<ChainMonitorType>,
    channel_manager: NodeChannelManager,
    esplora: Arc<LexeEsplora>,
    wallet: LexeWallet,
    fee_estimates: Arc<FeeEstimatorType>,
    tx_broadcaster: Arc<BroadcasterType>,
    keys_manager: Arc<LexeKeysManager>,
    logger: LexeTracingLogger,
    network_graph: Arc<NetworkGraphType>,
    onion_messenger: Arc<OnionMessengerType>,
    payments_manager: PaymentsManagerType,
    peer_manager: NodePeerManager,
    persister: Arc<NodePersister>,
    router: Arc<LexeRouter>,
    scorer: Arc<Mutex<ProbabilisticScorerType>>,

    // --- Contexts --- //
    sync: Option<SyncContext>,
    run: Option<RunContext>,
}

/// Fields which are "moved" out of [`UserNode`] during `sync`.
struct SyncContext {
    init_start: Instant,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    onchain_recv_tx: notify::Sender,
    bdk_resync_rx: mpsc::Receiver<BdkSyncRequest>,
    ldk_resync_rx: mpsc::Receiver<oneshot::Sender<()>>,
    user_ready_waiter_rx:
        mpsc::Receiver<oneshot::Sender<Result<RunPorts, MegaApiError>>>,
}

/// Fields which are "moved" out of [`UserNode`] during `run`.
struct RunContext {
    eph_tasks_rx: mpsc::Receiver<LxTask<()>>,
}

impl UserNode {
    // TODO(max): We can speed up initializing all the LDK actors by separating
    // into two stages: (1) fetch and (2) deserialize. Optimistically fetch all
    // the data in ~one roundtrip to the API, and then deserialize the data in
    // the required order.
    pub async fn init(
        rng: &mut impl Crng,
        args: RunArgs,
        mega_ctxt: MegaContext,
        user_ctxt: UserContext,
    ) -> anyhow::Result<Self> {
        info!(%args.user_pk, "Initializing node");
        let init_start = Instant::now();

        let MegaContext {
            backend_api,
            config,
            gdrive_oauth_config,
            esplora,
            fee_estimates,
            logger,
            lsp_api,
            machine_id,
            measurement,
            network_graph,
            runner_api,
            runner_tx,
            scorer,
            untrusted_deploy_env,
            untrusted_network,
            version,
        } = mega_ctxt.clone();

        // Get user_pk
        let user_pk = args.user_pk;

        // Init channels
        let (gdrive_persister_tx, gdrive_persister_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (channel_monitor_persister_tx, channel_monitor_persister_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (bdk_resync_tx, bdk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (ldk_resync_tx, ldk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (test_event_tx, test_event_rx) = test_event::channel("(node)");
        let test_event_rx = Arc::new(tokio::sync::Mutex::new(test_event_rx));
        let (eph_tasks_tx, eph_tasks_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = user_ctxt.user_shutdown;

        // Fetch provisioned secrets
        let ProvisionedSecrets {
            user,
            root_seed,
            deploy_env,
            network,
            user_key_pair,
            node_key_pair: _,
        } = fetch_provisioned_secrets(
            backend_api.as_ref(),
            user_pk,
            measurement,
            machine_id,
        )
        .await
        .context("Failed to fetch provisioned secrets")?;

        // Validate deploy env and network
        if deploy_env.is_staging_or_prod() && cfg!(feature = "test-utils") {
            panic!("test-utils feature must be disabled in staging/prod!!");
        }
        ensure!(
            untrusted_deploy_env == deploy_env,
            "Mismatched deploy envs: {untrusted_deploy_env} != {deploy_env}"
        );
        ensure!(
            network == untrusted_network,
            "Unsealed network didn't match network from MegaContext: \
            {network}!={untrusted_network}",
        );
        // From here, `deploy_env` and `network` can be treated as trusted.

        let mut static_tasks = Vec::new();

        // If we're in staging or prod, init a GoogleVfs.
        let authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));
        let vfs_master_key = Arc::new(root_seed.derive_vfs_master_key());

        let (maybe_google_vfs, gdrive_status) = if deploy_env
            .is_staging_or_prod()
        {
            let gvfs_root_name = GvfsRootName {
                deploy_env,
                network,
                use_sgx: cfg!(target_env = "sgx"),
                user_pk,
            };

            let maybe_gvfs_and_task = gdrive_provision::maybe_init_google_vfs(
                backend_api.clone(),
                authenticator.clone(),
                vfs_master_key.clone(),
                gvfs_root_name,
                shutdown.clone(),
            )
            .await;

            match maybe_gvfs_and_task {
                Ok(None) => (None, GDriveStatus::Disabled),
                Ok(Some((google_vfs, credentials_persister_task))) => {
                    static_tasks.push(credentials_persister_task);
                    (Some(Arc::new(google_vfs)), GDriveStatus::Ok)
                }
                Err(GoogleVfsInitError::VfsInit(e)) =>
                    (None, GDriveStatus::Error(e.to_string())),
                Err(GoogleVfsInitError::FetchCreds(e)) => bail!(e),
                Err(GoogleVfsInitError::PersistRoot(e)) => bail!(e),
            }
        } else {
            (None, GDriveStatus::Disabled)
        };

        // Initialize Persister
        let persister = Arc::new(NodePersister::new(
            backend_api.clone(),
            authenticator.clone(),
            vfs_master_key.clone(),
            maybe_google_vfs.clone(),
            channel_monitor_persister_tx,
            gdrive_persister_tx.clone(),
            eph_tasks_tx.clone(),
            shutdown.clone(),
        ));

        // A future which reads the approved versions list
        let read_maybe_approved_versions = persister::read_approved_versions(
            &backend_api,
            &authenticator,
            &vfs_master_key,
        );

        // Read as much as possible concurrently to reduce init time
        #[rustfmt::skip] // Does not respect 80 char line width
        let (
            try_maybe_approved_versions,
            try_maybe_changeset,
            try_existing_scids,
            try_pending_payments,
            try_finalized_payment_ids,
            try_maybe_revocable_clients,
            try_channel_monitor_bytes,
        ) = tokio::join!(
            read_maybe_approved_versions,
            persister.read_wallet_changeset(),
            persister.read_scids(),
            persister.read_pending_payments(),
            persister.read_finalized_payment_ids(),
            persister.read_json::<RevocableClients>(&REVOCABLE_CLIENTS_FILE_ID),
            persister.fetch_channel_monitor_bytes(),
        );
        if deploy_env.is_staging_or_prod() {
            // Erroring here prevents an attacker with access to a target user's
            // gdrive from deleting the user's approved versions list in an
            // attempt to roll back the user to an older vulnerable version.
            let approved_versions = try_maybe_approved_versions
                .context("Couldn't read approved versions")?
                .context(
                    "No approved versions list found; we'll assume that \
                     *nothing* has been approved; shutting down.",
                )?;
            let current_version = semver::Version::parse(SEMVER_VERSION)
                .expect("Checked in approved_versions tests");
            let approved_measurement =
                approved_versions.approved.get(&current_version).context(
                    "Current version not found in approved versions list; \
                     we are not authorized to run; shutting down.",
                )?;
            ensure!(
                *approved_measurement == measurement,
                "Current measurement doesn't match approved measurement: \
                {approved_measurement}",
            );
        }
        let maybe_changeset =
            try_maybe_changeset.context("Could not read wallet changeset")?;
        let existing_scids =
            try_existing_scids.context("Could not read scid")?;
        let intercept_scids = if existing_scids.len() < MIN_INTERCEPT_SCIDS {
            // We don't have enough scids; ask the LSP to give us enough.
            let req = GetNewScidsRequest {
                node_pk: user.node_pk,
                min_scids: MIN_INTERCEPT_SCIDS,
            };

            let scids_from_lsp = lsp_api
                .get_new_scids(&req)
                .await
                .context("Could not get new scid from LSP")?
                .scids;

            if scids_from_lsp.len() < MIN_INTERCEPT_SCIDS {
                warn!("LSP didn't give us enough scids; using what we have");
            }

            scids_from_lsp
        } else {
            existing_scids
        };
        let pending_payments =
            try_pending_payments.context("Could not read pending payments")?;
        let finalized_payment_ids = try_finalized_payment_ids
            .context("Could not read finalized payment ids")?;
        let revocable_clients = try_maybe_revocable_clients
            .context("Could not read revocable clients")?
            .unwrap_or_default()
            .apply(RwLock::new)
            .apply(Arc::new);

        // Create a fresh EsploraSyncClient for this user node. The sync client
        // maintains internal state and cannot be shared between nodes, though
        // we can share the underlying LexeEsplora connection pool.
        let ldk_sync_client = Arc::new(EsploraSyncClient::from_client(
            esplora.client().clone(),
            logger.clone(),
        ));

        // Init BDK wallet; share esplora connection pool, spawn persister task
        let (wallet_persister_tx, wallet_persister_rx) = notify::channel();
        let coin_selector = LexeCoinSelector::default();
        let wallet = LexeWallet::init(
            &root_seed,
            network,
            &esplora,
            fee_estimates.clone(),
            coin_selector,
            maybe_changeset,
            wallet_persister_tx,
        )
        .await
        .context("Could not init BDK wallet")?;
        static_tasks.push(wallet::spawn_wallet_persister_task(
            persister.clone(),
            wallet.clone(),
            wallet_persister_rx,
            shutdown.clone(),
        ));

        // Init tx broadcaster with a hook which logs into VFS all broadcasted
        // txs.
        let broadcast_hook = {
            let persister = persister.clone();
            Arc::new(move |tx: &bitcoin::Transaction| {
                let persister = persister.clone();
                let txid = tx.compute_txid();

                let mut tx_buf = Vec::new();
                let encoded_result = tx
                    .consensus_encode(&mut tx_buf)
                    .context("Failed to consensus encode bitcoin tx");

                Box::pin(async move {
                    encoded_result?;

                    let broadcasted_tx =
                        BroadcastedTx::new(LxTxid(txid), tx_buf);

                    let file_id = VfsFileId::new(
                        vfs::BROADCASTED_TXS_DIR,
                        txid.to_string(),
                    );

                    debug!("Persisting broadcasted tx");
                    let file = persister.encrypt_json(file_id, &broadcasted_tx);
                    let retries = 1;
                    persister
                        .persist_file(file, retries)
                        .await
                        .context("Failed to persist broadcasted tx")?;

                    Ok::<_, anyhow::Error>(())
                }) as BoxedAnyhowFuture
            })
        };

        let (tx_broadcaster, broadcaster_task) = TxBroadcaster::start(
            esplora.clone(),
            wallet.clone(),
            Some(broadcast_hook),
            test_event_tx.clone(),
            shutdown.clone(),
        );
        static_tasks.push(broadcaster_task);

        // Initialize the chain monitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(ldk_sync_client.clone()),
            tx_broadcaster.clone(),
            logger.clone(),
            fee_estimates.clone(),
            persister.clone(),
        ));

        // Init keys manager.
        let keys_manager =
            LexeKeysManager::new(rng, &root_seed, wallet.clone())
                .map(Arc::new)
                .context("Failed to construct keys manager")?;

        // Deserialize channel monitors from previously fetched bytes
        let channel_monitor_bytes = try_channel_monitor_bytes
            .context("Could not fetch channel monitor bytes")?;
        let mut channel_monitors = NodePersister::deserialize_channel_monitors(
            channel_monitor_bytes,
            &keys_manager,
        )
        .context("Could not deserialize channel monitors")?;

        // Initialize Router
        let router = Arc::new(LexeRouter::new_user_node(
            network_graph.clone(),
            logger.clone(),
            scorer.clone(),
            args.lsp.clone(),
            intercept_scids.clone(),
        ));

        // Read channel manager
        let message_router = Arc::new(LexeMessageRouter::new_user_node(
            network_graph.clone(),
            args.lsp.clone(),
        ));
        let maybe_manager = persister
            .read_channel_manager(
                *config,
                &mut channel_monitors,
                keys_manager.clone(),
                fee_estimates.clone(),
                chain_monitor.clone(),
                tx_broadcaster.clone(),
                router.clone(),
                message_router.clone(),
                logger.clone(),
            )
            .await
            .context("Could not read channel manager")?;

        // Init the NodeChannelManager
        let channel_manager = NodeChannelManager::init(
            network,
            *config,
            maybe_manager,
            keys_manager.clone(),
            fee_estimates.clone(),
            chain_monitor.clone(),
            tx_broadcaster.clone(),
            router.clone(),
            message_router.clone(),
            logger.clone(),
        )
        .context("Could not init NodeChannelManager")?;

        // Move the channel monitors into the chain monitor so that it can watch
        // the chain for closing transactions, fraudulent transactions, etc.
        for (_blockhash, monitor) in channel_monitors {
            let (funding_txo, _script) = monitor.get_funding_txo();
            let counterparty_node_id = monitor
                .get_counterparty_node_id()
                .expect("Launched after v0.0.110");

            // Method docs indicate that if this `Err`s, we should immediately
            // force close without broadcasting the funding txn.
            // No one else seems to do this though...
            if let Err(()) = chain_monitor.watch_channel(funding_txo, monitor) {
                let channel_id =
                    ChannelId::v1_from_funding_outpoint(funding_txo);
                warn!(
                    %channel_id, %funding_txo,
                    "`ChainMonitor::watch_channel` failed; force closing..."
                );

                channel_manager
                    .force_close_without_broadcasting_txn(
                        &channel_id,
                        &counterparty_node_id,
                        "Couldn't watch this channel".to_owned(),
                    )
                    .inspect(|()| {
                        info!(
                            %channel_id, %funding_txo,
                            "Successfully force closed"
                        )
                    })
                    .map_err(|e| {
                        let funding_txo = LxOutPoint::from(funding_txo);
                        anyhow!(
                            "Couldn't force close bad monitor: {e:?} \
                             channel_id='{channel_id}', \
                             funding_txo='{funding_txo}'"
                        )
                    })?;
            }
        }

        // Init onion messenger
        let offers_msg_handler = channel_manager.clone();
        let async_payments_msg_handler = IgnoringMessageHandler {};
        let dns_resolver = IgnoringMessageHandler {};
        let custom_onion_msg_handler = IgnoringMessageHandler {};
        let onion_messenger = Arc::new(LexeOnionMessengerType::new(
            keys_manager.clone(),
            keys_manager.clone(),
            logger.clone(),
            channel_manager.clone(),
            message_router,
            offers_msg_handler,
            async_payments_msg_handler,
            dns_resolver,
            custom_onion_msg_handler,
        ));

        // Initialize PeerManager
        let routing_msg_handler = Arc::new(IgnoringMessageHandler {});
        let (peer_manager, process_events_task) = NodePeerManager::init(
            rng,
            keys_manager.clone(),
            channel_manager.clone(),
            routing_msg_handler,
            onion_messenger.clone(),
            logger.clone(),
            shutdown.clone(),
        );
        static_tasks.push(process_events_task);

        // Init payments manager
        let (onchain_recv_tx, onchain_recv_rx) = notify::channel();
        let (payments_manager, payments_tasks) = PaymentsManager::new(
            persister.clone(),
            channel_manager.clone(),
            esplora.clone(),
            pending_payments,
            finalized_payment_ids,
            wallet.clone(),
            onchain_recv_rx,
            test_event_tx.clone(),
            shutdown.clone(),
        );
        static_tasks.extend(payments_tasks);

        // Initialize the event handler
        let channel_events_bus = EventsBus::new();
        let htlcs_forwarded_bus = EventsBus::new();
        let event_handler = NodeEventHandler {
            ctx: Arc::new(event_handler::EventCtx {
                user_pk,
                lsp: args.lsp.clone(),
                lsp_api: lsp_api.clone(),
                persister: persister.clone(),
                fee_estimates: fee_estimates.clone(),
                tx_broadcaster: tx_broadcaster.clone(),
                wallet: wallet.clone(),
                channel_manager: channel_manager.clone(),
                keys_manager: keys_manager.clone(),
                network_graph: network_graph.clone(),
                scorer: scorer.clone(),
                payments_manager: payments_manager.clone(),

                channel_events_bus: channel_events_bus.clone(),
                eph_tasks_tx: eph_tasks_tx.clone(),
                gdrive_persister_tx,
                htlcs_forwarded_bus: htlcs_forwarded_bus.clone(),
                runner_tx: runner_tx.clone(),
                test_event_tx: test_event_tx.clone(),
                shutdown: shutdown.clone(),
            }),
        };

        // Spawn task to replay any unhandled events
        static_tasks
            .push(event::spawn_event_replayer_task(event_handler.clone()));

        // Set up the channel monitor persistence task
        let monitor_persister_shutdown = NotifyOnce::new();
        let gdrive_persister_shutdown = NotifyOnce::new();
        let max_active_persists = 4;
        let task = ChannelMonitorPersister::new(
            persister.clone(),
            channel_manager.clone(),
            chain_monitor.clone(),
            channel_monitor_persister_rx,
            shutdown.clone(),
            monitor_persister_shutdown.clone(),
            Some(gdrive_persister_shutdown.clone()),
            max_active_persists,
        )
        .spawn();
        static_tasks.push(task);

        // GDrive persister task
        static_tasks.push(gdrive_persister::spawn_gdrive_persister_task(
            persister.clone(),
            gdrive_persister_rx,
            gdrive_persister_shutdown,
            shutdown.clone(),
        ));

        // Start API server for app
        let lsp_info = args.lsp.clone();
        let eph_ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let eph_ca_cert_der = eph_ca_cert
            .serialize_der_self_signed()
            .map(Arc::new)
            .context("Failed to serialize ephemeral issuing CA cert")?;
        let rev_ca_cert =
            Arc::new(RevocableIssuingCaCert::from_root_seed(&root_seed));
        let router_state = Arc::new(RouterState {
            // --- Info --- //
            user_pk,
            network,
            measurement,
            version: version.clone(),
            config: config.clone(),
            fee_estimates: fee_estimates.clone(),
            lsp_info: lsp_info.clone(),
            eph_ca_cert_der: eph_ca_cert_der.clone(),
            rev_ca_cert: rev_ca_cert.clone(),
            revocable_clients: revocable_clients.clone(),
            intercept_scids,
            gdrive_status: Arc::new(tokio::sync::Mutex::new(gdrive_status)),
            gdrive_oauth_config,
            deploy_env,
            // --- Actors --- //
            channel_manager: channel_manager.clone(),
            peer_manager: peer_manager.clone(),
            keys_manager: keys_manager.clone(),
            payments_manager: payments_manager.clone(),
            network_graph: network_graph.clone(),
            persister: persister.clone(),
            chain_monitor: chain_monitor.clone(),
            router: router.clone(),
            wallet: wallet.clone(),
            // --- Channels --- //
            tx_broadcaster: tx_broadcaster.clone(),
            channel_events_bus,
            eph_tasks_tx: eph_tasks_tx.clone(),
            runner_tx: runner_tx.clone(),
            bdk_resync_tx,
            ldk_resync_tx,
            test_event_rx,
            shutdown: shutdown.clone(),
        });
        let app_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind app listener")?;
        let app_port = app_listener
            .local_addr()
            .context("Couldn't get app addr")?
            .port();
        // `[preflight_]pay_invoice` may call `max_flow`.
        let app_layer_config = LayerConfig {
            handling_timeout: Some(constants::MAX_FLOW_TIMEOUT),
            ..Default::default()
        };
        let (app_tls_config, app_dns) =
            lexe_tls::shared_seed::node_run_server_config(
                rng,
                &eph_ca_cert,
                &eph_ca_cert_der,
                &rev_ca_cert,
                revocable_clients,
            )
            .context("Failed to build owner service TLS config")?;
        const APP_SERVER_SPAN_NAME: &str = "(app-node-run-server)";
        let (app_server_task, _app_url) =
            lexe_api::server::spawn_server_task_with_listener(
                app_listener,
                server::app_router(router_state.clone()),
                app_layer_config,
                Some((app_tls_config, &app_dns)),
                APP_SERVER_SPAN_NAME.into(),
                info_span!(APP_SERVER_SPAN_NAME),
                shutdown.clone(),
            )
            .context("Failed to spawn app node run server task")?;
        static_tasks.push(app_server_task);

        let lexe_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind lexe listener")?;
        let lexe_port = lexe_listener.local_addr()?.port();
        const LEXE_SERVER_SPAN_NAME: &str = "(lexe-node-run-server)";
        let lexe_tls_and_dns = None;
        let (lexe_server_task, _lexe_url) =
            lexe_api::server::spawn_server_task_with_listener(
                lexe_listener,
                server::lexe_router(router_state),
                LayerConfig::default(),
                lexe_tls_and_dns,
                LEXE_SERVER_SPAN_NAME.into(),
                info_span!(LEXE_SERVER_SPAN_NAME),
                shutdown.clone(),
            )
            .context("Failed to spawn lexe node run server task")?;
        static_tasks.push(lexe_server_task);

        // Prepare the ports that we'll notify the runner of once we're ready
        let run_ports = RunPorts {
            user_pk,
            app_port,
            lexe_port,
        };

        // Spawn a task which periodically logs the node's node_info.
        let node_info_task = {
            let lsp_info = lsp_info.clone();
            let channel_manager = channel_manager.clone();
            let peer_manager = peer_manager.clone();
            let wallet = wallet.clone();
            let chain_monitor = chain_monitor.clone();
            let mut shutdown = shutdown.clone();

            const SPAN_NAME: &str = "(node-info-logger)";
            LxTask::spawn_with_span(
                SPAN_NAME,
                info_span!(SPAN_NAME),
                async move {
                    const LOG_INTERVAL: Duration = Duration::from_secs(20);
                    let mut interval = tokio::time::interval(LOG_INTERVAL);

                    loop {
                        tokio::select! {
                            _ = interval.tick() => (),
                            () = shutdown.recv() => break,
                        }

                        let channels = channel_manager.list_channels();
                        let mut node_info = lexe_ln::command::node_info(
                            version.clone(),
                            measurement,
                            user_pk,
                            &channel_manager,
                            &peer_manager,
                            &wallet,
                            &chain_monitor,
                            &channels,
                            lsp_info.lsp_fees(),
                        );
                        // For privacy, zero out the on-chain balance so we
                        // don't leak this info in logs. Lexe can derive all of
                        // our LN balances by nature of being our LSP so there's
                        // no point in redacting the rest.
                        node_info.onchain_balance = OnchainBalance::ZERO;
                        let node_info_json = serde_json::to_string(&node_info)
                            .expect("Failed to serialize node info");
                        info!(
                            "Node info (on-chain zeroed out): {node_info_json}"
                        );
                    }
                },
            )
        };
        static_tasks.push(node_info_task);

        // Spawn lease renewal task
        const SPAN_NAME: &str = "(lease-renewer)";
        let lease_id = user_ctxt.lease_id;
        let lease_renewal_span = info_span!(SPAN_NAME, %user_pk, %lease_id);
        let lease_renewal_task =
            LxTask::spawn_with_span(SPAN_NAME, lease_renewal_span, {
                let user_pk = args.user_pk;

                let lease_renewal_interval =
                    Duration::from_secs(args.lease_renewal_interval_secs);
                let mut renewal_timer =
                    tokio::time::interval(lease_renewal_interval);

                let runner_api = runner_api.clone();
                let mut shutdown = shutdown.clone();

                async move {
                    loop {
                        tokio::select! {
                            _ = renewal_timer.tick() => {
                                debug!("Renewing lease");

                                let req = UserLeaseRenewalRequest {
                                    lease_id,
                                    user_pk,
                                    timestamp: TimestampMs::now(),
                                };

                                match runner_api.renew_lease(&req).await {
                                    Ok(_) => debug!("Success: renewed lease"),
                                    Err(e) => {
                                        error!("Failed to renew lease: {e:#}");
                                        // If we fail to renew the lease for any
                                        // reason, we should shut down.
                                        return shutdown.send();
                                    }
                                }
                            }
                            () = shutdown.recv() => return,
                        }
                    }
                }
            });
        static_tasks.push(lease_renewal_task);

        // Init background processor. User nodes can't be observed from the
        // outside, so there is no point in having any forwarding delay.
        let forward_delay_range_ms = 0..=0;
        let bg_processor_task = background_processor::start(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            event_handler,
            forward_delay_range_ms,
            htlcs_forwarded_bus,
            monitor_persister_shutdown,
            shutdown.clone(),
        );
        static_tasks.push(bg_processor_task);

        // Ensure channels are using the most up-to-date config.
        channel_manager.check_channel_configs(&config);

        let elapsed = init_start.elapsed().as_millis();
        info!("Node initialization complete. <{elapsed}ms>");

        // Build and return the UserNode
        Ok(Self {
            // General
            args,
            deploy_env,
            run_ports,
            static_tasks,
            eph_tasks_tx,
            shutdown,
            user_pk,
            runner_api,

            // Actors
            chain_monitor,
            channel_manager,
            esplora,
            wallet,
            fee_estimates,
            tx_broadcaster,
            keys_manager,
            logger,
            network_graph,
            onion_messenger,
            payments_manager,
            peer_manager,
            persister,
            router,
            scorer,

            // Contexts
            sync: Some(SyncContext {
                init_start,
                ldk_sync_client,
                onchain_recv_tx,
                bdk_resync_rx,
                ldk_resync_rx,
                user_ready_waiter_rx: user_ctxt.user_ready_waiter_rx,
            }),
            run: Some(RunContext { eph_tasks_rx }),
        })
    }

    pub async fn sync(&mut self) -> anyhow::Result<()> {
        info!("Starting sync");
        let ctxt = self.sync.take().expect("sync() must be called only once");

        // BDK: Do initial wallet sync
        let (first_bdk_sync_tx, first_bdk_sync_rx) = oneshot::channel();
        self.static_tasks.push(sync::spawn_bdk_sync_task(
            self.esplora.clone(),
            self.wallet.clone(),
            ctxt.onchain_recv_tx,
            first_bdk_sync_tx,
            ctxt.bdk_resync_rx,
            self.shutdown.clone(),
        ));
        let bdk_sync_fut = first_bdk_sync_rx
            .map(|res| res.context("Failed to recv result of first BDK sync"));

        // LDK: Do initial tx sync
        let (first_ldk_sync_tx, first_ldk_sync_rx) = oneshot::channel();
        self.static_tasks.push(sync::spawn_ldk_sync_task(
            self.channel_manager.clone(),
            self.chain_monitor.clone(),
            ctxt.ldk_sync_client,
            first_ldk_sync_tx,
            ctxt.ldk_resync_rx,
            self.shutdown.clone(),
        ));
        let ldk_sync_fut = first_ldk_sync_rx
            .map(|res| res.context("Failed to recv result of first LDK sync"));

        // Sync BDK and LDK concurrently
        let (try_first_bdk_sync, try_first_ldk_sync) =
            tokio::try_join!(bdk_sync_fut, ldk_sync_fut)?;
        try_first_bdk_sync.context("Initial BDK sync failed")?;
        try_first_ldk_sync.context("Initial LDK sync failed")?;

        // Notify runner of our successful sync.
        // We spawn in a task so as not to delay the ready callback.
        let sync_succ_task = {
            let runner_api = self.runner_api.clone();
            let user_pk = self.user_pk;

            const SPAN_NAME: &str = "(sync-success-notify)";
            LxTask::spawn_with_span(
                SPAN_NAME,
                info_span!(SPAN_NAME),
                async move {
                    match runner_api.sync_succ(user_pk).await {
                        Ok(_) => debug!("Notified runner of successful sync"),
                        Err(e) => warn!("Failed to notify sync success: {e:#}"),
                    }
                },
            )
        };
        let _ = self.eph_tasks_tx.send(sync_succ_task).await;

        // Reconnect to Lexe's LSP.
        // We only reconnect to the LSP *after* we have completed init + sync,
        // as it's our signal to the LSP that we are ready to receive messages.
        let maybe_connector_task = maybe_reconnect_to_lsp(
            self.peer_manager.clone(),
            &self.args.lsp,
            self.eph_tasks_tx.clone(),
            self.shutdown.clone(),
        )
        .await
        .context("maybe_reconnect_to_lsp failed")?;
        if let MaybeLxTask(Some(connector_task)) = maybe_connector_task {
            self.static_tasks.push(connector_task);
        }

        // Spawn a task which simply responds with `RunPorts` when asked.
        //
        // NOTE: It is important that we tell the notify the `user_ready_waiter`
        // only *after* we have reconnected to Lexe's LSP (just above).
        //
        // This is because the LSP's HTLCIntercepted event handler might be
        // waiting on the MegaRunner which is waiting on the UserRunner, with
        // the intention of opening a JIT channel with us as soon as soon as the
        // usernode is ready. Thus, to ensure that the LSP is connected to us
        // when it makes its open_channel request, we reconnect to the LSP
        // *before* sending the /ready callback.
        let ports_responder_task = {
            let run_ports = self.run_ports;
            let mut user_ready_waiter_rx = ctxt.user_ready_waiter_rx;
            let mut shutdown = self.shutdown.clone();

            const SPAN_NAME: &str = "(ports-responder)";
            LxTask::spawn_with_span(
                SPAN_NAME,
                info_span!(SPAN_NAME),
                async move {
                    loop {
                        tokio::select! {
                            biased;
                            Some(user_ready_waiter) =
                                user_ready_waiter_rx.recv() => {
                                let _ = user_ready_waiter.send(Ok(run_ports));
                            }
                            () = shutdown.recv() => return,
                        }
                    }
                },
            )
        };
        self.static_tasks.push(ports_responder_task);

        let total_elapsed = ctxt.init_start.elapsed().as_millis();
        info!("Sync complete. Total init + sync time: <{total_elapsed}ms>");

        Ok(())
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("Running...");
        assert!(self.sync.is_none(), "Must sync before run");
        let ctxt = self.run.take().expect("run() must be called only once");

        // Sync complete. Trigger shutdown if we were asked to do so after sync.
        if self.args.shutdown_after_sync {
            self.shutdown.send();
        }

        // --- Run --- //

        const_assert!(
            constants::USER_NODE_SHUTDOWN_TIMEOUT.as_secs()
                > lexe_api::server::SERVER_SHUTDOWN_TIMEOUT.as_secs()
        );

        task::try_join_tasks_and_shutdown(
            self.static_tasks,
            ctxt.eph_tasks_rx,
            self.shutdown.clone(),
            constants::USER_NODE_SHUTDOWN_TIMEOUT,
        )
        .await
        .context("Error awaiting tasks")?;

        Ok(())
    }
}

struct ProvisionedSecrets {
    user: User,
    root_seed: RootSeed,
    deploy_env: DeployEnv,
    network: LxNetwork,
    user_key_pair: ed25519::KeyPair,
    #[allow(unused)] // May be used to generate `NodePkProof`s later
    node_key_pair: secp256k1::Keypair,
}

/// Fetches and validates previously provisioned secrets from the API.
// Really this could just take `&dyn NodeBackendApi` but dyn upcasting is
// marked as incomplete and not yet safe to use as of 2023-02-01.
// https://github.com/rust-lang/rust/issues/65991
async fn fetch_provisioned_secrets(
    backend_api: &NodeBackendClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
) -> anyhow::Result<ProvisionedSecrets> {
    debug!(%user_pk, %measurement, %machine_id, "fetching provisioned secrets");
    let mut rng = SysRng::new();

    let sealed_seed_id = SealedSeedId {
        user_pk,
        measurement,
        machine_id,
    };

    let (try_maybe_user, try_maybe_sealed_seed) = tokio::join!(
        backend_api.get_user(user_pk),
        backend_api.get_sealed_seed(&sealed_seed_id)
    );

    let maybe_user = try_maybe_user.context("Error while fetching user")?;
    let maybe_sealed_seed =
        try_maybe_sealed_seed.context("Error while fetching sealed seed")?;

    match (maybe_user.maybe_user, maybe_sealed_seed.maybe_seed) {
        (Some(user), Some(sealed_seed)) => {
            let db_user_pk = user.user_pk;
            let db_node_pk = user.node_pk;
            ensure!(
                db_user_pk == user_pk,
                "UserPk {db_user_pk} from DB didn't match {user_pk} from CLI"
            );

            let (root_seed, deploy_env, unsealed_network) = sealed_seed
                .unseal_and_validate(&measurement, &machine_id)
                .context("Could not validate or unseal sealed seed")?;

            let user_key_pair = root_seed.derive_user_key_pair();
            let derived_user_pk =
                UserPk::from_ref(user_key_pair.public_key().as_inner());
            let derived_node_key_pair =
                root_seed.derive_node_key_pair(&mut rng);
            let derived_node_pk = NodePk(derived_node_key_pair.public_key());

            ensure!(
                &user_pk == derived_user_pk,
                "The user_pk derived from the sealed seed {derived_user_pk} \
                doesn't match the user_pk from CLI {user_pk}"
            );
            ensure!(
                db_node_pk == derived_node_pk,
                "The node_pk derived from the sealed seed {derived_node_pk} \
                doesn't match the node_pk from CLI {db_node_pk}"
            );

            Ok(ProvisionedSecrets {
                user,
                root_seed,
                deploy_env,
                network: unsealed_network,
                user_key_pair,
                node_key_pair: derived_node_key_pair,
            })
        }
        (None, None) => bail!("User does not exist yet"),
        (Some(_), None) => bail!(
            "User account exists but this node version is not provisioned yet"
        ),
        (None, Some(_)) => bail!(
            "CORRUPT: somehow the User does not exist but this user node is \
             provisioned!!!"
        ),
    }
}

/// Spawns the task which reconnects to Lexe's LSP, notifying our p2p
/// reconnector to continuously reconnect if we disconnect for some reason.
async fn maybe_reconnect_to_lsp(
    peer_manager: NodePeerManager,
    lsp: &LspInfo,
    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    shutdown: NotifyOnce,
) -> anyhow::Result<MaybeLxTask<()>> {
    info!("Spawning LSP connector task");
    let task = p2p::connect_to_lsp_then_spawn_connector_task(
        peer_manager,
        lsp,
        eph_tasks_tx,
        shutdown,
    )
    .await
    .context("connect_to_lsp_then_spawn_connector_task failed")?;

    Ok(MaybeLxTask(Some(task)))
}
