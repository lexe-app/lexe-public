use std::{
    net::TcpListener,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, ensure, Context};
use common::{
    aes::AesMasterKey,
    api::{
        auth::BearerAuthenticator, def::NodeRunnerApi, ports::Ports,
        provision::SealedSeedId, server::LayerConfig, User, UserPk,
    },
    cli::{node::RunArgs, LspInfo, Network},
    constants::{DEFAULT_CHANNEL_SIZE, SMALLER_CHANNEL_SIZE},
    ed25519,
    enclave::{self, MachineId, Measurement, MIN_SGX_CPUSVN},
    env::DeployEnv,
    net, notify,
    rng::{Crng, SysRng},
    root_seed::RootSeed,
    shutdown::ShutdownChannel,
    task::{self, LxTask},
    tls, Apply,
};
use futures::{
    future::FutureExt,
    stream::{FuturesUnordered, StreamExt},
};
use gdrive::GoogleVfs;
use lexe_ln::{
    alias::{
        BroadcasterType, EsploraSyncClientType, FeeEstimatorType,
        NetworkGraphType, OnionMessengerType, P2PGossipSyncType,
        ProbabilisticScorerType, RouterType,
    },
    background_processor::LexeBackgroundProcessor,
    channel_monitor,
    esplora::LexeEsplora,
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    p2p,
    p2p::ChannelPeerUpdate,
    payments::manager::PaymentsManager,
    sync, test_event,
    traits::LexeInnerPersister,
    wallet::{self, LexeWallet},
};
use lightning::{
    chain::{chainmonitor::ChainMonitor, Watch},
    ln::peer_handler::IgnoringMessageHandler,
    onion_message::{DefaultMessageRouter, OnionMessenger},
    routing::{
        gossip::P2PGossipSync, router::DefaultRouter,
        scoring::ProbabilisticScoringFeeParameters,
    },
    sign::EntropySource,
};
use lightning_transaction_sync::EsploraSyncClient;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, info_span, instrument, warn};

use crate::{
    alias::{ChainMonitorType, NodePaymentsManagerType},
    api::{self, BackendApiClient},
    channel_manager::NodeChannelManager,
    event_handler::NodeEventHandler,
    inactivity_timer::InactivityTimer,
    peer_manager::NodePeerManager,
    persister::{self, NodePersister},
    server::{self, AppRouterState, LexeRouterState},
    DEV_VERSION, SEMVER_VERSION,
};

// TODO(max): Move this to common::constants
/// The amount of time tasks have to finish after a graceful shutdown was
/// initiated before the program exits.
const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(15);

/// A user's node.
#[allow(dead_code)] // Many unread fields are used as type annotations
pub struct UserNode {
    // --- General --- //
    args: RunArgs,
    deploy_env: DeployEnv,
    ports: Ports,
    tasks: Vec<LxTask<()>>,
    channel_peer_tx: mpsc::Sender<ChannelPeerUpdate>,
    shutdown: ShutdownChannel,

    // --- Actors --- //
    logger: LexeTracingLogger,
    persister: Arc<NodePersister>,
    wallet: LexeWallet,
    fee_estimator: Arc<FeeEstimatorType>,
    broadcaster: Arc<BroadcasterType>,
    esplora: Arc<LexeEsplora>,
    keys_manager: Arc<LexeKeysManager>,
    chain_monitor: Arc<ChainMonitorType>,
    network_graph: Arc<NetworkGraphType>,
    gossip_sync: Arc<P2PGossipSyncType>,
    scorer: Arc<Mutex<ProbabilisticScorerType>>,
    router: Arc<RouterType>,
    channel_manager: NodeChannelManager,
    onion_messenger: Arc<OnionMessengerType>,
    peer_manager: NodePeerManager,
    inactivity_timer: InactivityTimer,
    payments_manager: NodePaymentsManagerType,

    // --- Contexts --- //
    sync: Option<SyncContext>,
}

/// Fields which are "moved" out of [`UserNode`] during `sync`.
struct SyncContext {
    runner_api: Arc<dyn NodeRunnerApi + Send + Sync>,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    init_start: Instant,
    onchain_recv_tx: notify::Sender,
    bdk_resync_rx: mpsc::Receiver<oneshot::Sender<()>>,
    ldk_resync_rx: mpsc::Receiver<oneshot::Sender<()>>,
}

impl UserNode {
    // TODO(max): We can speed up initializing all the LDK actors by separating
    // into two stages: (1) fetch and (2) deserialize. Optimistically fetch all
    // the data in ~one roundtrip to the API, and then deserialize the data in
    // the required order.
    #[instrument(skip_all, name = "(node)")]
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: RunArgs,
    ) -> anyhow::Result<Self> {
        info!(%args.user_pk, "Initializing node");
        let init_start = Instant::now();

        // Initialize the Logger
        let logger = LexeTracingLogger::new();

        // Get user_pk, measurement, and HTTP clients used throughout init
        let user_pk = args.user_pk;
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        // TODO(phlip9): Compare this with current cpusvn
        let _min_cpusvn = MIN_SGX_CPUSVN;
        let backend_api = api::new_backend_api(
            args.allow_mock,
            args.untrusted_deploy_env,
            args.backend_url.clone(),
        )
        .context("Failed to init dyn BackendApiClient")?;

        // Init channels
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (channel_monitor_persister_tx, channel_monitor_persister_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (channel_peer_tx, channel_peer_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (bdk_resync_tx, bdk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (ldk_resync_tx, ldk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (test_event_tx, test_event_rx) = test_event::channel("(node)");
        let test_event_rx = Arc::new(tokio::sync::Mutex::new(test_event_rx));
        let shutdown = ShutdownChannel::new();

        // Version
        let version = DEV_VERSION
            .unwrap_or(SEMVER_VERSION)
            .apply(semver::Version::parse)
            .expect("Checked in tests");

        // Collect all handles to spawned tasks
        let mut tasks = Vec::with_capacity(10);

        // Initialize esplora while fetching provisioned secrets
        let (try_esplora, try_fetch) = tokio::join!(
            LexeEsplora::init(
                args.esplora_url.clone(),
                test_event_tx.clone(),
                shutdown.clone()
            ),
            fetch_provisioned_secrets(
                backend_api.as_ref(),
                user_pk,
                measurement,
                machine_id,
            ),
        );
        let (esplora, refresh_fees_task) =
            try_esplora.context("Failed to init esplora")?;
        tasks.push(refresh_fees_task);
        let (user, root_seed, deploy_env, network, user_key_pair) =
            try_fetch.context("Failed to fetch provisioned secrets")?;

        // Validate deploy env and network
        if deploy_env.is_staging_or_prod() && cfg!(feature = "test-utils") {
            panic!("test-utils feature must be disabled in staging/prod!!");
        }
        let args_deploy_env = args.untrusted_deploy_env;
        ensure!(
            args_deploy_env == deploy_env,
            "Mismatched deploy envs: {args_deploy_env} != {deploy_env}"
        );
        let args_network = args.network;
        ensure!(
            network == args_network,
            "Unsealed network didn't match network given by CLI: \
            {network}!={args_network}",
        );
        // From here, `deploy_env` and `network` can be treated as trusted.

        // Init the remaining API clients
        let runner_api = api::new_runner_api(
            args.allow_mock,
            deploy_env,
            args.runner_url.clone(),
        )
        .context("Failed to init dyn NodeRunnerApi")?;
        let lsp_api = api::new_lsp_api(
            args.allow_mock,
            deploy_env,
            args.lsp.url.clone(),
        )?;

        // Validate esplora url
        let esplora_url = &args.esplora_url;
        info!(%esplora_url);
        network
            .validate_esplora_url(esplora_url)
            .context("Invalid esplora url")?;

        // Init LDK transaction sync; share LexeEsplora's connection pool
        // XXX(max): The esplora url passed to LDK is security-critical and thus
        // should use Blockstream.info when `Network` is `Mainnet`.
        let ldk_sync_client = Arc::new(EsploraSyncClient::from_client(
            esplora.client().clone(),
            logger.clone(),
        ));

        // Clone FeeEstimator and BroadcasterInterface impls
        let fee_estimator = esplora.clone();
        let broadcaster = esplora.clone();

        // If we're in staging or prod, init a GoogleVfs.
        let authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));
        let vfs_master_key = Arc::new(root_seed.derive_vfs_master_key());
        let maybe_google_vfs = if deploy_env.is_staging_or_prod() {
            let (google_vfs, credentials_persister_task) = init_google_vfs(
                backend_api.clone(),
                authenticator.clone(),
                vfs_master_key.clone(),
                network,
                shutdown.clone(),
            )
            .await
            .context("init_google_vfs failed")?;
            tasks.push(credentials_persister_task);
            Some(Arc::new(google_vfs))
        } else {
            None
        };

        // Initialize Persister
        let persister = Arc::new(NodePersister::new(
            backend_api.clone(),
            authenticator,
            vfs_master_key.clone(),
            maybe_google_vfs.clone(),
            user,
            shutdown.clone(),
            channel_monitor_persister_tx,
        ));

        // Initialize the chain monitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(ldk_sync_client.clone()),
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            persister.clone(),
        ));

        // A closure to read the approved versions list if we have a gvfs.
        let read_maybe_approved_versions = async {
            let google_vfs = match maybe_google_vfs {
                None => return Ok(None),
                Some(ref gvfs) => gvfs,
            };
            persister::read_approved_versions(google_vfs, &vfs_master_key).await
        };

        // Read as much as possible concurrently to reduce init time
        let (wallet_db_persister_tx, wallet_db_persister_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        #[rustfmt::skip] // Does not respect 80 char line width
        let (
            try_maybe_approved_versions,
            try_network_graph,
            try_wallet_db,
            try_scid,
            try_pending_payments,
            try_finalized_payment_ids,
        ) = tokio::join!(
            read_maybe_approved_versions,
            persister.read_network_graph(network, logger.clone()),
            persister.read_wallet_db(wallet_db_persister_tx),
            persister.read_scid(),
            persister.read_pending_payments(),
            persister.read_finalized_payment_ids(),
        );
        if deploy_env.is_staging_or_prod() {
            let maybe_approved_versions = try_maybe_approved_versions
                .context("Couldn't read approved versions")?;
            // Erroring here prevents an attacker with access to a target user's
            // gdrive from deleting the user's approved versions list in an
            // attempt to roll back the user to an older vulnerable version.
            let approved_versions = maybe_approved_versions.context(
                "No approved versions list found; \
                 for safety we'll assume that *nothing* has been approved; \
                 shutting down.",
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
        let network_graph = try_network_graph
            .map(Arc::new)
            .context("Could not read network graph")?;
        let wallet_db = try_wallet_db.context("Could not read wallet db")?;
        let maybe_scid = try_scid.context("Could not read scid")?;
        let scid = match maybe_scid {
            Some(s) => s,
            // We has not been assigned an scid yet; ask the LSP for one
            None => lsp_api
                .get_new_scid(user.node_pk)
                .await
                .context("Could not get new scid from LSP")?,
        };
        let pending_payments =
            try_pending_payments.context("Could not read pending payments")?;
        let finalized_payment_ids = try_finalized_payment_ids
            .context("Could not read finalized payment ids")?;

        // Init BDK wallet; share esplora connection pool, spawn persister task
        let wallet = LexeWallet::new(
            &root_seed,
            network,
            esplora.clone(),
            wallet_db.clone(),
        )
        .context("Could not init BDK wallet")?;
        tasks.push(wallet::spawn_wallet_db_persister_task(
            persister.clone(),
            wallet_db,
            wallet_db_persister_rx,
            shutdown.clone(),
        ));

        // Init gossip sync
        let gossip_sync = Arc::new(P2PGossipSync::new(
            network_graph.clone(),
            None,
            logger.clone(),
        ));

        // Init keys manager. NOTE: If a user sends to their on-chain wallet
        // then closes a channel in the same node run, there will be address
        // reuse. This is the quickest way to work around the non-async
        // SignerProvider for now, but we should fix this eventually.
        let recv_address = wallet
            .get_address()
            .await
            .context("Could not get receive address")?;
        let keys_manager =
            LexeKeysManager::init(rng, &user.node_pk, &root_seed, recv_address)
                .context("Failed to construct keys manager")?
                .apply(Arc::new);

        // Read channel monitors and scorer
        let (try_channel_monitors, try_scorer) = tokio::join!(
            persister.read_channel_monitors(keys_manager.clone()),
            persister.read_scorer(network_graph.clone(), logger.clone()),
        );
        let mut channel_monitors =
            try_channel_monitors.context("Could not read channel monitors")?;
        let scorer = try_scorer
            .context("Could not read probabilistic scorer")?
            .apply(Mutex::new)
            .apply(Arc::new);

        // Initialize Router
        let scoring_fee_params = ProbabilisticScoringFeeParameters::default();
        let router = Arc::new(DefaultRouter::new(
            network_graph.clone(),
            logger.clone(),
            keys_manager.get_secure_random_bytes(),
            scorer.clone(),
            scoring_fee_params,
        ));

        // Read channel manager
        let maybe_manager = persister
            .read_channel_manager(
                &mut channel_monitors,
                keys_manager.clone(),
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                router.clone(),
                logger.clone(),
            )
            .await
            .context("Could not read channel manager")?;

        // Init the NodeChannelManager
        let channel_manager = NodeChannelManager::init(
            network,
            maybe_manager,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            router.clone(),
            logger.clone(),
        )
        .context("Could not init NodeChannelManager")?;

        // Move the channel monitors into the chain monitor so that it can watch
        // the chain for closing transactions, fraudulent transactions, etc.
        for (_blockhash, monitor) in channel_monitors {
            let (funding_txo, _script) = monitor.get_funding_txo();
            chain_monitor.watch_channel(funding_txo, monitor);
        }

        // Init onion messenger
        let onion_messenger = Arc::new(OnionMessenger::new(
            keys_manager.clone(),
            keys_manager.clone(),
            logger.clone(),
            Arc::new(DefaultMessageRouter {}),
            IgnoringMessageHandler {},
            IgnoringMessageHandler {},
        ));

        // Initialize PeerManager
        let peer_manager = NodePeerManager::init(
            rng,
            keys_manager.clone(),
            channel_manager.clone(),
            gossip_sync.clone(),
            onion_messenger.clone(),
            logger.clone(),
        );

        // The LSP is the only peer the p2p reconnector needs to reconnect to,
        // but we do so only *after* we have completed init and sync; it is our
        // signal to the LSP that we are ready to receive messages.
        let initial_channel_peers = Vec::new();

        // Spawn the task to regularly reconnect to channel peers
        tasks.push(p2p::spawn_p2p_reconnector(
            peer_manager.clone(),
            initial_channel_peers,
            channel_peer_rx,
            shutdown.clone(),
        ));

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
        tasks.extend(payments_tasks);

        // Initialize the event handler
        let fatal_event = Arc::new(AtomicBool::new(false));
        let event_handler = NodeEventHandler {
            lsp: args.lsp.clone(),
            wallet: wallet.clone(),
            channel_manager: channel_manager.clone(),
            keys_manager: keys_manager.clone(),
            esplora: esplora.clone(),
            network_graph: network_graph.clone(),
            payments_manager: payments_manager.clone(),
            fatal_event: fatal_event.clone(),
            test_event_tx: test_event_tx.clone(),
            shutdown: shutdown.clone(),
        };

        // Set up the channel monitor persistence task
        let (process_events_tx, process_events_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        tasks.push(channel_monitor::spawn_channel_monitor_persister_task(
            chain_monitor.clone(),
            channel_monitor_persister_rx,
            process_events_tx,
            shutdown.clone(),
        ));

        // Start API server for app
        let app_router_state = Arc::new(AppRouterState {
            version,
            persister: persister.clone(),
            chain_monitor: chain_monitor.clone(),
            wallet: wallet.clone(),
            esplora: esplora.clone(),
            router: router.clone(),
            channel_manager: channel_manager.clone(),
            peer_manager: peer_manager.clone(),
            keys_manager: keys_manager.clone(),
            payments_manager: payments_manager.clone(),
            lsp_info: args.lsp.clone(),
            scid,
            network,
            measurement,
            activity_tx,
        });
        let app_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind app listener")?;
        let app_port = app_listener
            .local_addr()
            .context("Couldn't get app addr")?
            .port();
        let (app_tls_config, app_dns) =
            tls::shared_seed::app_node_run_server_config(rng, &root_seed)
                .context("Failed to build owner service TLS config")?;
        const APP_SERVER_SPAN_NAME: &str = "(app-node-run-server)";
        let (app_server_task, _app_url) =
            common::api::server::spawn_server_task_with_listener(
                app_listener,
                server::app_router(app_router_state),
                LayerConfig::default(),
                Some((Arc::new(app_tls_config), app_dns.as_str())),
                APP_SERVER_SPAN_NAME,
                info_span!(parent: None, APP_SERVER_SPAN_NAME),
                shutdown.clone(),
            )
            .context("Failed to spawn app node run server task")?;
        tasks.push(app_server_task);

        // Start API server for Lexe operators
        // TODO(phlip9): authenticate lexe<->node
        let lexe_router_state = Arc::new(LexeRouterState {
            user_pk: args.user_pk,
            channel_manager: channel_manager.clone(),
            peer_manager: peer_manager.clone(),
            lsp_info: args.lsp.clone(),
            bdk_resync_tx,
            ldk_resync_tx,
            test_event_rx,
            shutdown: shutdown.clone(),
        });
        let lexe_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind lexe listener")?;
        let lexe_port = lexe_listener.local_addr()?.port();
        const LEXE_SERVER_SPAN_NAME: &str = "(lexe-node-run-server)";
        let lexe_tls_and_dns = None;
        let (lexe_server_task, _lexe_url) =
            common::api::server::spawn_server_task_with_listener(
                lexe_listener,
                server::lexe_router(lexe_router_state),
                LayerConfig::default(),
                lexe_tls_and_dns,
                LEXE_SERVER_SPAN_NAME,
                info_span!(parent: None, LEXE_SERVER_SPAN_NAME),
                shutdown.clone(),
            )
            .context("Failed to spawn lexe node run server task")?;
        tasks.push(lexe_server_task);

        // Prepare the ports that we'll notify the runner of once we're ready
        let ports = Ports::new_run(user_pk, app_port, lexe_port);

        // Init background processor
        let bg_processor_task = LexeBackgroundProcessor::start::<
            NodeChannelManager,
            NodePeerManager,
            Arc<NodePersister>,
            NodeEventHandler,
        >(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            event_handler,
            gossip_sync.clone(),
            scorer.clone(),
            process_events_rx,
            fatal_event,
            shutdown.clone(),
        );
        tasks.push(bg_processor_task);

        // Construct (but don't start) the inactivity timer
        let inactivity_timer = InactivityTimer::new(
            args.shutdown_after_sync,
            args.inactivity_timer_sec,
            activity_rx,
            shutdown.clone(),
        );

        let elapsed = init_start.elapsed().as_millis();
        info!("Node initialization complete. <{elapsed}ms>");

        // Build and return the UserNode
        Ok(Self {
            // General
            args,
            deploy_env,
            ports,
            tasks,
            channel_peer_tx,
            shutdown,

            // Actors
            logger,
            persister,
            wallet,
            fee_estimator,
            broadcaster,
            esplora,
            keys_manager,
            chain_monitor,
            network_graph,
            gossip_sync,
            scorer,
            router,
            channel_manager,
            onion_messenger,
            peer_manager,
            inactivity_timer,
            payments_manager,

            // Contexts
            sync: Some(SyncContext {
                runner_api,
                ldk_sync_client,
                init_start,
                onchain_recv_tx,
                bdk_resync_rx,
                ldk_resync_rx,
            }),
        })
    }

    #[instrument(skip_all, name = "(node)")]
    pub async fn sync(&mut self) -> anyhow::Result<()> {
        info!("Starting sync");
        let ctxt = self.sync.take().expect("sync() must be called only once");

        // BDK: Do initial wallet sync
        let (first_bdk_sync_tx, first_bdk_sync_rx) = oneshot::channel();
        self.tasks.push(sync::spawn_bdk_sync_task(
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
        self.tasks.push(sync::spawn_ldk_sync_task(
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

        // Reconnect to Lexe's LSP.
        maybe_reconnect_to_lsp(
            &self.peer_manager,
            self.deploy_env,
            self.args.allow_mock,
            &self.args.lsp,
            &self.channel_peer_tx,
        )
        .await
        .context("Could not reconnect to LSP")?;

        // NOTE: It is important that we tell the runner that we're ready only
        // *after* we have successfully reconnected to Lexe's LSP (just above).
        // This is because the LSP might be waiting on the runner in its handler
        // for the HTLCIntercepted event, with the intention of opening a JIT
        // channel with us as soon as soon as we are ready. Thus, to ensure that
        // the LSP is connected to us when it makes its open_channel request, we
        // reconnect to the LSP *before* sending the /ready callback.
        ctxt.runner_api
            .ready(&self.ports)
            .await
            .context("Could not notify runner of ready status")?;

        let total_elapsed = ctxt.init_start.elapsed().as_millis();
        info!("Sync complete. Total init + sync time: <{total_elapsed}ms>");

        Ok(())
    }

    #[instrument(skip_all, name = "(node)")]
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("Running...");
        assert!(self.sync.is_none(), "Must sync before run");

        // Sync is complete; start the inactivity timer.
        debug!("Starting inactivity timer");
        self.tasks
            .push(LxTask::spawn_named("inactivity timer", async move {
                self.inactivity_timer.start().await;
            }));

        // --- Run --- //

        let mut tasks = self
            .tasks
            .into_iter()
            .map(|task| task.with_name())
            .collect::<FuturesUnordered<_>>();

        // Wait for a shutdown signal and poll all tasks so we can (1) propagate
        // panics and (2) detect if a task finished prematurely, in which case a
        // [partial] failure occurred and we should shut down.
        tokio::select! {
            // Mitigate possible select! race after a shutdown signal is sent
            biased;
            () = self.shutdown.recv() => (),
            Some(output) = tasks.next() => {
                task::log_finished_task(&output, true);
                self.shutdown.send();
            }
        }

        // --- Shutdown --- //
        info!("Received shutdown; disconnecting all peers");
        // This ensures we don't continue updating our channel data after
        // the background processor has stopped.
        self.peer_manager.disconnect_all_peers();

        info!("Waiting on all tasks to finish");
        let timeout = tokio::time::sleep(SHUTDOWN_TIME_LIMIT);
        tokio::pin!(timeout);
        while !tasks.is_empty() {
            tokio::select! {
                () = &mut timeout => {
                    let stuck_tasks = tasks
                        .iter()
                        .map(|task| task.name())
                        .collect::<Vec<_>>();

                    // TODO(phlip9): is there some way to get a backtrace of a
                    //               stuck task?

                    error!("{} tasks failed to finish: {stuck_tasks:?}", stuck_tasks.len());
                    break;
                }
                Some(output) = tasks.next() =>
                    task::log_finished_task(&output, false),
            }
        }

        Ok(())
    }
}

/// Fetches previously provisioned secrets from the API.
// Really this could just take `&dyn NodeBackendApi` but dyn upcasting is
// marked as incomplete and not yet safe to use as of 2023-02-01.
// https://github.com/rust-lang/rust/issues/65991
async fn fetch_provisioned_secrets(
    backend_api: &dyn BackendApiClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
) -> anyhow::Result<(User, RootSeed, DeployEnv, Network, ed25519::KeyPair)> {
    debug!(%user_pk, %measurement, %machine_id, "fetching provisioned secrets");

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

    match (maybe_user, maybe_sealed_seed) {
        (Some(user), Some(sealed_seed)) => {
            let db_user_pk = user.user_pk;
            ensure!(
                user_pk == db_user_pk,
                "UserPk {db_user_pk} from DB didn't match {user_pk} from CLI"
            );

            let (root_seed, deploy_env, unsealed_network) = sealed_seed
                .unseal_and_validate(&measurement, &machine_id)
                .context("Could not validate or unseal sealed seed")?;

            let user_key_pair = root_seed.derive_user_key_pair();
            let derived_user_pk =
                UserPk::from_ref(user_key_pair.public_key().as_inner());

            ensure!(
                &user_pk == derived_user_pk,
                "The user_pk derived from the sealed seed {derived_user_pk} \
                doesn't match the user_pk from CLI {user_pk} "
            );

            Ok((user, root_seed, deploy_env, unsealed_network, user_key_pair))
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

/// Helper to efficiently initialize a [`GoogleVfs`] and handle related work.
/// Also spawns a task which persists updated GDrive credentials.
async fn init_google_vfs(
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    network: Network,
    mut shutdown: ShutdownChannel,
) -> anyhow::Result<(GoogleVfs, LxTask<()>)> {
    // Fetch the encrypted GDriveCredentials and persisted GVFS root.
    let (try_gdrive_credentials, try_persisted_gvfs_root) = tokio::join!(
        persister::read_gdrive_credentials(
            &*backend_api,
            &authenticator,
            &vfs_master_key,
        ),
        persister::read_gvfs_root(
            &*backend_api,
            &authenticator,
            &vfs_master_key
        ),
    );
    let gdrive_credentials =
        try_gdrive_credentials.context("Could not read GDrive credentials")?;
    let persisted_gvfs_root =
        try_persisted_gvfs_root.context("Could not read gvfs root")?;

    let (google_vfs, maybe_new_gvfs_root, mut credentials_rx) =
        GoogleVfs::init(gdrive_credentials, network, persisted_gvfs_root)
            .await
            .context("Failed to init Google VFS")?;

    // If we were given a new GVFS root to persist, persist it.
    // This should only happen once so it won't impact startup time.
    let mut rng = SysRng::new();
    if let Some(new_gvfs_root) = maybe_new_gvfs_root {
        persister::persist_gvfs_root(
            &mut rng,
            &*backend_api,
            &authenticator,
            &vfs_master_key,
            &new_gvfs_root,
        )
        .await
        .context("Failed to persist new GVFS root")?;
    }

    // Spawn a task that repersists the GDriveCredentials every time
    // the contained access token is updated.
    let credentials_persister_task =
        LxTask::spawn_named("gdrive credentials persister", async move {
            loop {
                tokio::select! {
                    Ok(()) = credentials_rx.changed() => {
                        let credentials_file =
                            persister::encrypt_gdrive_credentials(
                                &mut rng,
                                &vfs_master_key,
                                &credentials_rx.borrow_and_update(),
                            );

                        let try_persist = persister::persist_file(
                            &*backend_api,
                            &authenticator,
                            &credentials_file,
                        )
                        .await;

                        match try_persist {
                            Ok(()) => debug!(
                                "Successfully persisted updated credentials"
                            ),
                            Err(e) => warn!(
                                "Failed to persist updated credentials: {e:#}"
                            ),
                        }
                    }
                    () = shutdown.recv() => break,
                }
            }
            info!("gdrive credentials persister task shutting down");
        });

    Ok((google_vfs, credentials_persister_task))
}

/// Handles the logic of whether to reconnect to Lexe's LSP, taking in account
/// whether we are intend to mock out the LSP as well.
///
/// If we are on testnet/mainnet, mocking out the LSP is not allowed. Ignore all
/// mock arguments and attempt to reconnect to Lexe's LSP, notifying our p2p
/// reconnector to continuously reconnect if we disconnect for some reason.
///
/// If we are NOT on testnet/mainnet, we MAY skip reconnecting to the LSP.
/// This will be done ONLY IF [`LspInfo::url`] is `None` AND we have set the
/// `allow_mock` safeguard which helps prevent accidental mocking.
async fn maybe_reconnect_to_lsp(
    peer_manager: &NodePeerManager,
    deploy_env: DeployEnv,
    allow_mock: bool,
    lsp: &LspInfo,
    channel_peer_tx: &mpsc::Sender<ChannelPeerUpdate>,
) -> anyhow::Result<()> {
    if deploy_env.is_staging_or_prod() || lsp.url.is_some() {
        // If --allow-mock was set, the caller may have made an error.
        ensure!(
            !allow_mock,
            "--allow-mock was set but a LSP url was supplied"
        );

        info!("Reconnecting to LSP");
        p2p::connect_channel_peer_if_necessary(
            peer_manager.clone(),
            lsp.channel_peer(),
        )
        .await
        .context("Could not connect to LSP")?;

        debug!("Notifying reconnector task of LSP channel peer");
        let add_lsp = ChannelPeerUpdate::Add(lsp.channel_peer());
        channel_peer_tx
            .try_send(add_lsp)
            .map_err(|e| anyhow!("Could not notify p2p reconnector: {e:#}"))?;
    } else {
        ensure!(allow_mock, "To mock the LSP, allow_mock must be set");
        info!("Skipping P2P reconnection to LSP");
    }

    Ok(())
}
