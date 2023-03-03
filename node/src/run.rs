use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use common::api::auth::UserAuthenticator;
use common::api::def::NodeRunnerApi;
use common::api::ports::UserPorts;
use common::api::provision::SealedSeedId;
use common::api::{User, UserPk};
use common::cli::node::RunArgs;
use common::client::tls::node_run_tls_config;
use common::constants::{DEFAULT_CHANNEL_SIZE, SMALLER_CHANNEL_SIZE};
use common::enclave::{
    self, MachineId, Measurement, MinCpusvn, MIN_SGX_CPUSVN,
};
use common::rng::Crng;
use common::root_seed::RootSeed;
use common::shutdown::ShutdownChannel;
use common::task::{joined_task_state_label, BlockingTaskRt, LxTask};
use common::{ed25519, notify};
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use lexe_ln::alias::{
    BroadcasterType, EsploraSyncClientType, FeeEstimatorType, NetworkGraphType,
    OnionMessengerType, P2PGossipSyncType, PaymentInfoStorageType,
    ProbabilisticScorerType,
};
use lexe_ln::background_processor::LexeBackgroundProcessor;
use lexe_ln::esplora::LexeEsplora;
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lexe_ln::p2p::ChannelPeerUpdate;
use lexe_ln::test_event::TestEventSender;
use lexe_ln::wallet::{self, LexeWallet};
use lexe_ln::{channel_monitor, p2p, sync};
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::EntropySource;
use lightning::ln::peer_handler::IgnoringMessageHandler;
use lightning::onion_message::OnionMessenger;
use lightning::routing::gossip::P2PGossipSync;
use lightning::routing::router::DefaultRouter;
use lightning_transaction_sync::EsploraSyncClient;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, instrument};

use crate::alias::ChainMonitorType;
use crate::api::BackendApiClient;
use crate::channel_manager::NodeChannelManager;
use crate::event_handler::NodeEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;
use crate::{api, server};

// TODO(max): Move this to common::constants
/// The amount of time tasks have to finish after a graceful shutdown was
/// initiated before the program exits.
const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(15);

/// A user's node.
#[allow(dead_code)] // Many unread fields are used as type annotations
pub struct UserNode {
    // --- General --- //
    args: RunArgs,
    pub(crate) user_ports: UserPorts,
    tasks: Vec<LxTask<()>>,
    channel_peer_tx: mpsc::Sender<ChannelPeerUpdate>,
    shutdown: ShutdownChannel,

    // --- Actors --- //
    pub logger: LexeTracingLogger,
    pub persister: NodePersister,
    pub wallet: LexeWallet,
    fee_estimator: Arc<FeeEstimatorType>,
    broadcaster: Arc<BroadcasterType>,
    esplora: Arc<LexeEsplora>,
    pub keys_manager: LexeKeysManager,
    chain_monitor: Arc<ChainMonitorType>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    gossip_sync: Arc<P2PGossipSyncType>,
    scorer: Arc<Mutex<ProbabilisticScorerType>>,
    pub channel_manager: NodeChannelManager,
    onion_messenger: Arc<OnionMessengerType>,
    pub peer_manager: NodePeerManager,
    inactivity_timer: InactivityTimer,
    pub outbound_payments: PaymentInfoStorageType,

    // --- Contexts --- //
    sync: Option<SyncContext>,
}

/// Fields which are "moved" out of [`UserNode`] during `sync`.
struct SyncContext {
    runner_api: Arc<dyn NodeRunnerApi + Send + Sync>,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    resync_rx: watch::Receiver<()>,
    test_event_tx: TestEventSender,
}

impl UserNode {
    // TODO(max): We can speed up initializing all the LDK actors by separating
    // into two stages: (1) fetch and (2) deserialize. Optimistically fetch all
    // the data in ~one roundtrip to the API, and then deserialize the data in
    // the required order.
    #[instrument(skip_all, name = "[node]")]
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: RunArgs,
        resync_rx: watch::Receiver<()>,
        // TODO(max): The `process_events` channel should be created during
        // init instead of passed in, but currently cannot be due to smoketest
        // constraints. Fix the integration tests to allow this, then remove.
        process_events_tx: notify::Sender,
        process_events_rx: notify::Receiver,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<Self> {
        info!(%args.user_pk, "Initializing node");

        // Initialize the Logger
        let logger = LexeTracingLogger::new();

        // Get user_pk, measurement, and HTTP client, used throughout init
        let user_pk = args.user_pk;
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = MIN_SGX_CPUSVN;
        let backend_api =
            api::new_backend_api(args.allow_mock, args.backend_url.clone())?;
        let runner_api =
            api::new_runner_api(args.allow_mock, args.runner_url.clone())?;
        let lsp_api = api::new_lsp_api(args.allow_mock, args.lsp.url.clone())?;

        // Init Tokio channels
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (channel_monitor_persister_tx, channel_monitor_persister_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (channel_peer_tx, channel_peer_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);

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
                min_cpusvn
            ),
        );
        let (esplora, refresh_fees_task) =
            try_esplora.context("Failed to init esplora")?;
        tasks.push(refresh_fees_task);
        let (user, root_seed, user_key_pair) =
            try_fetch.context("Failed to fetch provisioned secrets")?;

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

        // Build LexeKeysManager from node init data
        let keys_manager =
            LexeKeysManager::init(rng, &user.node_pk, &root_seed)
                .context("Failed to construct keys manager")?;

        // Initialize Persister
        let authenticator =
            Arc::new(UserAuthenticator::new(user_key_pair, None));
        let vfs_master_key = Arc::new(root_seed.derive_vfs_master_key());
        let persister = NodePersister::new(
            backend_api.clone(),
            authenticator,
            vfs_master_key,
            user_pk,
            shutdown.clone(),
            channel_monitor_persister_tx,
        );

        // Initialize the chain monitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(ldk_sync_client.clone()),
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            persister.clone(),
        ));

        // Read channel monitors, network graph, wallet db, and scid
        let (wallet_db_persister_tx, wallet_db_persister_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        #[rustfmt::skip] // Does not respect 80 char line width
        let (try_channel_monitors, try_network_graph, try_wallet_db, try_scid) =
            tokio::join!(
                persister.read_channel_monitors(keys_manager.clone()),
                persister.read_network_graph(args.network, logger.clone()),
                persister.read_wallet_db(wallet_db_persister_tx),
                backend_api.get_scid(user.node_pk),
            );
        let mut channel_monitors =
            try_channel_monitors.context("Could not read channel monitors")?;
        let network_graph = try_network_graph
            .map(Arc::new)
            .context("Could not read network graph")?;
        let wallet_db = try_wallet_db.context("Could not read wallet db")?;
        let maybe_scid = try_scid.context("Could not read scid")?;
        // TODO(max): Pass the scid into the get_invoice handler
        let _scid = match maybe_scid {
            Some(s) => s,
            // We has not been assigned an scid yet; ask the LSP for one
            None => lsp_api
                .get_new_scid(user.node_pk)
                .await
                .context("Could not get new scid from LSP")?,
        };

        // Init BDK wallet; share esplora connection pool, spawn persister task
        let wallet = LexeWallet::new(
            &root_seed,
            args.network,
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

        // Init scorer
        let scorer = persister
            .read_scorer(network_graph.clone(), logger.clone())
            .await
            .map(Mutex::new)
            .map(Arc::new)
            .context("Could not read probabilistic scorer")?;

        // Initialize Router
        let router = Arc::new(DefaultRouter::new(
            network_graph.clone(),
            logger.clone(),
            keys_manager.get_secure_random_bytes(),
            scorer.clone(),
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
            args.network,
            maybe_manager,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            router.clone(),
            logger.clone(),
        )
        .context("Could not init NodeChannelManager")?;

        // Init onion messenger
        let onion_messenger = Arc::new(OnionMessenger::new(
            keys_manager.clone(),
            keys_manager.clone(),
            logger.clone(),
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

        // Set up listening for inbound P2P connections
        let (listener, peer_port) = {
            // A value of 0 indicates that the OS will assign a port for us
            // TODO(phlip9): user nodes should only listen on internal
            // interface. LSP should should accept external connections
            let address = format!("0.0.0.0:{}", args.peer_port.unwrap_or(0));
            let listener = TcpListener::bind(address)
                .await
                .context("Failed to bind to peer port")?;
            let peer_port = listener.local_addr().unwrap().port();
            (listener, peer_port)
        };
        info!("Listening for LN P2P connections on port {peer_port}");
        tasks.push(p2p::spawn_p2p_listener(
            listener,
            peer_manager.clone(),
            shutdown.clone(),
        ));

        // The LSP is the only peer the p2p reconnector needs to reconnect to,
        // but we do so only *after* we have completed init and sync; it is our
        // signal to the LSP that we are ready to receive messages.
        let initial_channel_peers = Vec::new();

        // Spawn the task to regularly reconnect to channel peers
        tasks.push(p2p::spawn_p2p_reconnector(
            channel_manager.clone(),
            peer_manager.clone(),
            initial_channel_peers,
            channel_peer_rx,
            shutdown.clone(),
        ));

        // Initialize the event handler
        // XXX(max): It is security-critical to persist our outbound payment
        // storage to ensure that we never pay the same `PaymentHash` twice.
        // See lightning_invoice::payments::pay_invoice or
        // ChannelManager::send_payment.
        let outbound_payments: PaymentInfoStorageType =
            Arc::new(Mutex::new(HashMap::new()));
        let event_handler = NodeEventHandler {
            lsp: args.lsp.clone(),
            wallet: wallet.clone(),
            channel_manager: channel_manager.clone(),
            keys_manager: keys_manager.clone(),
            esplora: esplora.clone(),
            network_graph: network_graph.clone(),
            outbound_payments: outbound_payments.clone(),
            test_event_tx: test_event_tx.clone(),
            blocking_task_rt: BlockingTaskRt::new(),
            shutdown: shutdown.clone(),
        };

        // Set up the channel monitor persistence task
        tasks.push(channel_monitor::spawn_channel_monitor_persister_task(
            chain_monitor.clone(),
            channel_monitor_persister_rx,
            process_events_tx.clone(),
            test_event_tx.clone(),
            shutdown.clone(),
        ));

        // Build owner service TLS config for authenticating owner
        let node_dns = args.node_dns_name.clone();
        let owner_tls = node_run_tls_config(rng, &root_seed, vec![node_dns])
            .context("Failed to build owner service TLS config")?;

        // Start warp service for owner
        let owner_routes = server::owner_routes(
            channel_manager.clone(),
            peer_manager.clone(),
            network_graph.clone(),
            keys_manager.clone(),
            outbound_payments.clone(),
            args.lsp.clone(),
            args.network,
            activity_tx,
            process_events_tx,
        );
        let mut owner_shutdown = shutdown.clone();
        let (owner_addr, owner_service_fut) = warp::serve(owner_routes)
            .tls()
            .preconfigured_tls(owner_tls)
            // A value of 0 indicates that the OS will assign a port for us
            .bind_with_graceful_shutdown(
                ([127, 0, 0, 1], args.owner_port.unwrap_or(0)),
                async move { owner_shutdown.recv().await },
            );
        let owner_port = owner_addr.port();
        info!("Owner service listening on port {}", owner_port);
        tasks.push(LxTask::spawn_named("owner service", owner_service_fut));

        // TODO(phlip9): authenticate host<->node
        // Start warp service for host
        let host_routes = server::host_routes(args.user_pk, shutdown.clone());
        let mut host_shutdown = shutdown.clone();
        let (host_addr, host_service_fut) = warp::serve(host_routes)
            // A value of 0 indicates that the OS will assign a port for us
            .try_bind_with_graceful_shutdown(
                ([127, 0, 0, 1], args.host_port.unwrap_or(0)),
                async move { host_shutdown.recv().await },
            )
            .context("Failed to bind warp")?;
        let host_port = host_addr.port();
        info!("Host service listening on port {}", host_port);
        tasks.push(LxTask::spawn_named("host service", host_service_fut));

        // Prepare the ports that we'll notify the runner of once we're ready
        let user_ports =
            UserPorts::new_run(user_pk, owner_port, host_port, peer_port);

        // Init background processor
        let bg_processor_task = LexeBackgroundProcessor::start::<
            NodeChannelManager,
            NodePeerManager,
            NodePersister,
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
            shutdown.clone(),
        );
        tasks.push(bg_processor_task);

        // Construct (but don't start) the inactivity timer
        let inactivity_timer = InactivityTimer::new(
            args.shutdown_after_sync_if_no_activity,
            args.inactivity_timer_sec,
            activity_rx,
            shutdown.clone(),
        );

        // Build and return the UserNode
        Ok(Self {
            // General
            args,
            user_ports,
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
            channel_manager,
            onion_messenger,
            peer_manager,
            inactivity_timer,

            // Storage
            outbound_payments,

            // Contexts
            sync: Some(SyncContext {
                runner_api,
                ldk_sync_client,
                resync_rx,
                test_event_tx,
            }),
        })
    }

    #[instrument(skip_all, name = "[node]")]
    pub async fn sync(&mut self) -> anyhow::Result<()> {
        info!("Starting sync");
        let ctxt = self.sync.take().expect("sync() must be called only once");

        // BDK: Do initial wallet sync
        let (first_bdk_sync_tx, first_bdk_sync_rx) = oneshot::channel();
        self.tasks.push(sync::spawn_bdk_sync_task(
            self.wallet.clone(),
            first_bdk_sync_tx,
            ctxt.resync_rx.clone(),
            ctxt.test_event_tx.clone(),
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
            ctxt.resync_rx,
            ctxt.test_event_tx,
            self.shutdown.clone(),
        ));
        let ldk_sync_fut = first_ldk_sync_rx
            .map(|res| res.context("Failed to recv result of first LDK sync"));

        // Sync BDK and LDK concurrently
        let (try_first_bdk_sync, try_first_ldk_sync) =
            tokio::try_join!(bdk_sync_fut, ldk_sync_fut)?;
        try_first_bdk_sync.context("Initial BDK sync failed")?;
        try_first_ldk_sync.context("Initial LDK sync failed")?;

        // Sync complete; let the runner know that we're ready
        info!("Node is ready to accept commands; notifying runner");
        ctxt.runner_api
            .ready(self.user_ports)
            .await
            .context("Could not notify runner of ready status")?;

        // We connect to the LSP only *after* we have completed init and sync;
        // it is our signal to the LSP that we are ready to receive messages.
        let add_lsp = ChannelPeerUpdate::Add(self.args.lsp.channel_peer());
        if let Err(e) = self.channel_peer_tx.try_send(add_lsp) {
            error!("Could not notify p2p reconnector to connect to LSP: {e:#}");
        }

        Ok(())
    }

    #[instrument(skip_all, name = "[node]")]
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
            .map(|task| task.result_with_name())
            .collect::<FuturesUnordered<_>>();

        while !tasks.is_empty() {
            tokio::select! {
                () = self.shutdown.recv() => break,
                // must poll tasks while waiting for shutdown, o/w a panic in a
                // task won't surface until later, when we start shutdown.
                Some((result, name)) = tasks.next() => {
                    let task_state = joined_task_state_label(result);
                    // tasks should probably only stop when we're shutting down.
                    info!("'{name}' task {task_state} before shutdown");
                }
            }
        }

        // --- Shutdown --- //
        info!("Recevied shutdown; disconnecting all peers");
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
                Some((result, name)) = tasks.next() => {
                    let task_state = joined_task_state_label(result);
                    info!("'{name}' task {task_state}");
                }
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
    min_cpusvn: MinCpusvn,
) -> anyhow::Result<(User, RootSeed, ed25519::KeyPair)> {
    debug!(%user_pk, %measurement, %machine_id, %min_cpusvn, "fetching provisioned secrets");

    let sealed_seed_id = SealedSeedId {
        user_pk,
        measurement,
        machine_id,
        min_cpusvn,
    };

    let (user_res, sealed_seed_res) = tokio::join!(
        backend_api.get_user(user_pk),
        backend_api.get_sealed_seed(sealed_seed_id)
    );

    let user_opt = user_res.context("Error while fetching user")?;
    let sealed_seed_opt =
        sealed_seed_res.context("Error while fetching sealed seed")?;

    match (user_opt, sealed_seed_opt) {
        (Some(user), Some(sealed_seed)) => {
            let root_seed = sealed_seed
                .unseal_and_validate(&measurement, &machine_id, &min_cpusvn)
                .context("Could not validate or unseal sealed seed")?;

            let user_key_pair = root_seed.derive_user_key_pair();
            let derived_user_pk =
                UserPk::from_ref(user_key_pair.public_key().as_inner());

            ensure!(
                &user.user_pk == derived_user_pk,
                "The user_pk derived from the sealed seed '{}' doesn't match \
                 the expected user_pk '{}'",
                derived_user_pk,
                user.user_pk,
            );

            Ok((user, root_seed, user_key_pair))
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
