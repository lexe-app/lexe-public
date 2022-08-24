use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::BlockHash;
use common::api::provision::{Node, ProvisionedSecrets, SealedSeedId};
use common::api::runner::{Port, UserPorts};
use common::api::UserPk;
use common::cli::{Network, RunArgs};
use common::client::tls::node_run_tls_config;
use common::enclave::{
    self, MachineId, Measurement, MinCpusvn, Sealed, MIN_SGX_CPUSVN,
};
use common::rng::Crng;
use futures::future;
use lightning::chain;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::chain::transaction::OutPoint;
use lightning::routing::gossip::P2PGossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio::time;
use tracing::{debug, error, info, instrument, warn};

use crate::api::ApiClient;
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::lexe::background_processor::LexeBackgroundProcessor;
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::{
    LexeChannelManager, LxChannelMonitorUpdate,
};
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::peer_manager::LexePeerManager;
use crate::lexe::persister::LexePersister;
use crate::lexe::sync::SyncedChainListeners;
use crate::types::{
    ApiClientType, BlockSourceType, BroadcasterType, ChainMonitorType,
    ChannelMonitorType, FeeEstimatorType, InvoicePayerType, LxTask,
    NetworkGraphType, P2PGossipSyncType, PaymentInfoStorageType, WalletType,
};
use crate::{api, command};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;
// TODO(max): p2p stuff should probably go in its own module
const P2P_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(15);

// TODO: Eventually move this into the `lexe` module once init is cleaned up
// TODO: Remove once keys_manager, persister, invoice_payer are read in SGX
#[allow(dead_code)]
pub struct LexeNode {
    // --- General --- //
    args: RunArgs,
    pub peer_port: Port,
    handles: Vec<LxTask<()>>,

    // --- Actors --- //
    pub channel_manager: LexeChannelManager,
    pub peer_manager: LexePeerManager,
    pub keys_manager: LexeKeysManager,
    pub persister: LexePersister,
    chain_monitor: Arc<ChainMonitorType>,
    pub network_graph: Arc<NetworkGraphType>,
    invoice_payer: Arc<InvoicePayerType>,
    pub wallet: Arc<WalletType>,
    block_source: Arc<BlockSourceType>,
    fee_estimator: Arc<FeeEstimatorType>,
    broadcaster: Arc<BroadcasterType>,
    logger: LexeTracingLogger,
    inactivity_timer: InactivityTimer,

    // --- Sync --- //
    restarting_node: bool,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    channel_manager_blockhash: BlockHash,

    // --- Run --- //
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
    spv_client_shutdown_rx: broadcast::Receiver<()>,
    main_shutdown_rx: broadcast::Receiver<()>,
}

impl LexeNode {
    #[instrument(skip_all)]
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: RunArgs,
    ) -> anyhow::Result<Self> {
        info!(%args.user_pk, "Initializing node");

        // Initialize the Logger
        let logger = LexeTracingLogger::new();

        // Get user_pk, measurement, and HTTP client, used throughout init
        let user_pk = args.user_pk;
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = MIN_SGX_CPUSVN;
        let api = init_api(&args);

        // Initialize LexeBitcoind, fetch provisioned data
        let (bitcoind_res, fetch_res) = tokio::join!(
            LexeBitcoind::init(args.bitcoind_rpc.clone(), args.network),
            fetch_provisioned_secrets(
                api.as_ref(),
                user_pk,
                measurement,
                machine_id,
                min_cpusvn
            ),
        );
        let bitcoind =
            bitcoind_res.context("Failed to init bitcoind client")?;
        let (node, provisioned_secrets) =
            fetch_res.context("Failed to fetch provisioned secrets")?;
        let root_seed = &provisioned_secrets.root_seed;
        let bitcoind = Arc::new(bitcoind);

        // Collect all handles to spawn tasks
        let mut handles = Vec::with_capacity(10);

        // Spawn task to refresh feerates
        let refresh_fees_handle = bitcoind.spawn_refresh_fees_task();
        handles.push(refresh_fees_handle);

        // Build LexeKeysManager from node init data
        let keys_manager = LexeKeysManager::init(rng, &node.node_pk, root_seed)
            .context("Failed to construct keys manager")?;
        let node_pk = keys_manager.derive_pk(rng);

        // LexeBitcoind implements BlockSource, FeeEstimator and
        // BroadcasterInterface, and thus serves these functions. It also
        // serves as the wallet for now. A type alias is defined for each of
        // these in case they need to be split apart later.
        let wallet = bitcoind.clone();
        let block_source = bitcoind.clone();
        let fee_estimator = bitcoind.clone();
        let broadcaster = bitcoind.clone();

        // Init Tokio channels
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = ShutdownChannel::init();
        let (channel_monitor_updated_tx, channel_monitor_updated_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);

        // Initialize Persister
        let persister = LexePersister::new(
            api.clone(),
            node_pk,
            measurement,
            channel_monitor_updated_tx,
        );

        // Initialize the ChainMonitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            None,
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            persister.clone(),
        ));

        // Set up the persister -> chain monitor channel
        let channel_monitor_updated_handle = spawn_channel_monitor_updated_task(
            chain_monitor.clone(),
            channel_monitor_updated_rx,
            shutdown.rx.channel_monitor_updated,
        );
        handles.push(channel_monitor_updated_handle);

        // Read the `ChannelMonitor`s and initialize the `P2PGossipSync`
        let (channel_monitors_res, gossip_sync_res) = tokio::join!(
            channel_monitors(&persister, keys_manager.clone()),
            gossip_sync(args.network, &persister, logger.clone())
        );
        let mut channel_monitors =
            channel_monitors_res.context("Could not read channel monitors")?;
        let (network_graph, gossip_sync) =
            gossip_sync_res.context("Could not initialize gossip sync")?;

        // Initialize the ChannelManager and ProbabilisticScorer
        let mut restarting_node = true;
        let (channel_manager_res, scorer_res) = tokio::join!(
            LexeChannelManager::init(
                &args,
                &persister,
                block_source.as_ref(),
                &mut restarting_node,
                &mut channel_monitors,
                keys_manager.clone(),
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                logger.clone(),
            ),
            persister.read_probabilistic_scorer(
                network_graph.clone(),
                logger.clone()
            ),
        );
        let (channel_manager_blockhash, channel_manager) =
            channel_manager_res.context("Could not init ChannelManager")?;
        let scorer =
            scorer_res.context("Could not read probabilistic scorer")?;
        let scorer = Arc::new(Mutex::new(scorer));

        // Initialize PeerManager
        let peer_manager = LexePeerManager::init(
            rng,
            &keys_manager,
            channel_manager.clone(),
            gossip_sync.clone(),
            logger.clone(),
        );

        // Set up listening for inbound P2P connections
        let (p2p_listener_handle, peer_port) = spawn_p2p_listener(
            peer_manager.clone(),
            args.peer_port,
            shutdown.rx.p2p_listener,
        )
        .await;
        handles.push(p2p_listener_handle);

        // Spawn a task to regularly reconnect to channel peers
        let p2p_reconnector_handle = spawn_p2p_reconnector(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            shutdown.rx.p2p_reconnector,
        );
        handles.push(p2p_reconnector_handle);

        // Build owner service TLS config for authenticating owner
        let node_dns = args.node_dns_name.clone();
        let owner_tls = node_run_tls_config(rng, root_seed, vec![node_dns])
            .context("Failed to build owner service TLS config")?;

        // Start warp service for owner
        let owner_routes = command::server::owner_routes(
            channel_manager.clone(),
            peer_manager.clone(),
            network_graph.clone(),
            activity_tx,
        );
        let (owner_addr, owner_service_fut) = warp::serve(owner_routes)
            .tls()
            .preconfigured_tls(owner_tls)
            // A value of 0 indicates that the OS will assign a port for us
            .bind_ephemeral(([127, 0, 0, 1], args.owner_port.unwrap_or(0)));
        let owner_port = owner_addr.port();
        info!("Owner service listening on port {}", owner_port);
        let owner_service_handle = LxTask::spawn(async move {
            owner_service_fut.await;
        });
        handles.push(owner_service_handle);

        // TODO(phlip9): authenticate host<->node
        // Start warp service for host
        let host_routes =
            command::server::host_routes(args.user_pk, shutdown.tx.host_routes);
        let (host_addr, host_service_fut) = warp::serve(host_routes)
            // A value of 0 indicates that the OS will assign a port for us
            .try_bind_ephemeral(([127, 0, 0, 1], args.host_port.unwrap_or(0)))
            .context("Failed to bind warp")?;
        let host_port = host_addr.port();
        info!("Host service listening on port {}", host_port);
        let host_service_handle = LxTask::spawn(async move {
            host_service_fut.await;
        });
        handles.push(host_service_handle);

        // Let the runner know that we're ready
        info!("Node is ready to accept commands; notifying runner");
        let user_ports =
            UserPorts::new_run(user_pk, owner_port, host_port, peer_port);
        api.ready(user_ports)
            .await
            .context("Could not notify runner of ready status")?;

        // Initialize the event handler
        // TODO: persist payment info
        let inbound_payments: PaymentInfoStorageType =
            Arc::new(Mutex::new(HashMap::new()));
        let outbound_payments: PaymentInfoStorageType =
            Arc::new(Mutex::new(HashMap::new()));
        let event_handler = LdkEventHandler::new(
            args.network,
            channel_manager.clone(),
            keys_manager.clone(),
            bitcoind.clone(),
            network_graph.clone(),
            inbound_payments.clone(),
            outbound_payments.clone(),
        );

        // Initialize InvoicePayer
        let router = DefaultRouter::new(
            network_graph.clone(),
            logger.clone(),
            keys_manager.get_secure_random_bytes(),
        );
        let invoice_payer = Arc::new(InvoicePayerType::new(
            channel_manager.clone(),
            router,
            scorer.clone(),
            logger.clone(),
            event_handler,
            payment::Retry::Timeout(Duration::from_secs(10)),
        ));

        // Start Background Processing
        let bg_processor_handle = LexeBackgroundProcessor::start(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            invoice_payer.clone(),
            gossip_sync.clone(),
            scorer.clone(),
            shutdown.tx.background_processor,
            shutdown.rx.background_processor,
        );
        handles.push(bg_processor_handle);

        // Construct (but don't start) the inactivity timer
        let inactivity_timer = InactivityTimer::new(
            args.shutdown_after_sync_if_no_activity,
            args.inactivity_timer_sec,
            activity_rx,
            shutdown.tx.inactivity_timer,
            shutdown.rx.inactivity_timer,
        );

        // Build and return the LexeNode
        let main_shutdown_rx = shutdown.rx.main;
        let spv_client_shutdown_rx = shutdown.rx.spv_client;
        let node = LexeNode {
            // General
            args,
            peer_port,
            handles,

            // Actors
            channel_manager,
            peer_manager,
            keys_manager,
            persister,
            chain_monitor,
            network_graph,
            invoice_payer,
            wallet,
            block_source,
            fee_estimator,
            broadcaster,
            logger,
            inactivity_timer,

            // Sync
            restarting_node,
            channel_manager_blockhash,
            channel_monitors,

            // Run
            inbound_payments,
            outbound_payments,
            spv_client_shutdown_rx,
            main_shutdown_rx,
        };
        Ok(node)
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        // --- Sync --- //

        // Sync channel manager and channel monitors to chain tip
        let synced_chain_listeners = SyncedChainListeners::init_and_sync(
            self.args.network,
            self.channel_manager.clone(),
            self.channel_manager_blockhash,
            self.channel_monitors,
            self.block_source.clone(),
            self.broadcaster.clone(),
            self.fee_estimator.clone(),
            self.logger.clone(),
            self.restarting_node,
        )
        .await
        .context("Could not sync channel listeners")?;

        // Populate / feed the chain monitor and spawn the SPV client
        let spv_client_handle = synced_chain_listeners
            .feed_chain_monitor_and_spawn_spv(
                self.chain_monitor.clone(),
                self.spv_client_shutdown_rx,
            )
            .context("Error wrapping up sync")?;
        self.handles.push(spv_client_handle);

        // Sync is complete; start the inactivity timer.
        debug!("Starting inactivity timer");
        let inactivity_timer_handle = LxTask::spawn(async move {
            self.inactivity_timer.start().await;
        });
        self.handles.push(inactivity_timer_handle);

        // --- Run --- //

        // Start the REPL if it was specified to start in the CLI args.
        #[cfg(not(target_env = "sgx"))]
        if self.args.repl {
            debug!("Starting REPL");
            crate::repl::poll_for_user_input(
                self.invoice_payer.clone(),
                self.peer_manager.clone(),
                self.channel_manager.clone(),
                self.keys_manager.clone(),
                self.network_graph.clone(),
                self.inbound_payments,
                self.outbound_payments,
                self.persister.clone(),
                self.args.network,
            )
            .await;
            debug!("REPL complete.");
        }

        // Pause here and wait for the shutdown signal
        let _ = self.main_shutdown_rx.recv().await;

        // --- Shutdown --- //
        info!("Main thread shutting down.");

        info!("Disconnecting all peers.");
        // This ensures we don't continue updating our channel data after we've
        // stopped the background processor.
        self.peer_manager.disconnect_all_peers();

        info!("Waiting on all tasks to finish");
        let join_all_with_timeout = time::timeout(
            SHUTDOWN_JOIN_TIMEOUT,
            future::join_all(self.handles),
        );
        match join_all_with_timeout.await {
            Ok(results) => {
                for res in results {
                    if let Err(e) = res {
                        error!("Spawned task panicked: {:#}", e);
                    }
                }
            }
            Err(e) => {
                error!("Joining on all spawned tasks timed out: {:#}", e);
            }
        }

        Ok(())
    }
}

/// Holds all the `shutdown_tx` / `shutdown_rx` channel senders and receivers
/// created during init. All senders / receivers are moved out of this struct at
/// some point during init.
struct ShutdownChannel {
    tx: ShutdownTx,
    rx: ShutdownRx,
}

struct ShutdownTx {
    host_routes: broadcast::Sender<()>,
    inactivity_timer: broadcast::Sender<()>,
    background_processor: broadcast::Sender<()>,
}

struct ShutdownRx {
    main: broadcast::Receiver<()>,
    spv_client: broadcast::Receiver<()>,
    p2p_listener: broadcast::Receiver<()>,
    p2p_reconnector: broadcast::Receiver<()>,
    inactivity_timer: broadcast::Receiver<()>,
    background_processor: broadcast::Receiver<()>,
    channel_monitor_updated: broadcast::Receiver<()>,
}

impl ShutdownChannel {
    /// Initializes all senders and receivers together to ensure that all calls
    /// to [`subscribe`] are complete before any values are sent (otherwise
    /// those values will not be received). This prevents race conditions where
    /// e.g. the inactivity timer sends on shutdown_tx before another task
    /// spawned during init has had a chance to subscribe.
    ///
    /// [`subscribe`]: broadcast::Sender::subscribe
    fn init() -> Self {
        let (shutdown_tx, _) = broadcast::channel(DEFAULT_CHANNEL_SIZE);

        // Clone txs
        let host_routes = shutdown_tx.clone();
        let inactivity_timer = shutdown_tx.clone();
        let background_processor = shutdown_tx.clone();
        let tx = ShutdownTx {
            host_routes,
            inactivity_timer,
            background_processor,
        };

        // Subscribe rxs
        let main = shutdown_tx.subscribe();
        let spv_client = shutdown_tx.subscribe();
        let p2p_listener = shutdown_tx.subscribe();
        let p2p_reconnector = shutdown_tx.subscribe();
        let inactivity_timer = shutdown_tx.subscribe();
        let background_processor = shutdown_tx.subscribe();
        let channel_monitor_updated = shutdown_tx.subscribe();
        let rx = ShutdownRx {
            main,
            spv_client,
            p2p_listener,
            p2p_reconnector,
            inactivity_timer,
            background_processor,
            channel_monitor_updated,
        };

        Self { tx, rx }
    }
}

/// Constructs a Arc<dyn ApiClient> based on whether we are running in SGX,
/// and whether `args.mock` is set to true
fn init_api(args: &RunArgs) -> ApiClientType {
    // Production can only use the real api client
    #[cfg(all(target_env = "sgx", not(test)))]
    {
        Arc::new(api::LexeApiClient::new(
            args.backend_url.clone(),
            args.runner_url.clone(),
        ))
    }
    // Development can use the real OR the mock client, depending on args.mock
    #[cfg(not(all(target_env = "sgx", not(test))))]
    {
        if args.mock {
            Arc::new(api::mock::MockApiClient::new())
        } else {
            Arc::new(api::LexeApiClient::new(
                args.backend_url.clone(),
                args.runner_url.clone(),
            ))
        }
    }
}

/// Fetches previously provisioned secrets from the API.
async fn fetch_provisioned_secrets(
    api: &dyn ApiClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
    min_cpusvn: MinCpusvn,
) -> anyhow::Result<(Node, ProvisionedSecrets)> {
    debug!(%user_pk, %measurement, %machine_id, %min_cpusvn, "fetching provisioned secrets");

    let (node_res, instance_res) = tokio::join!(
        api.get_node(user_pk),
        api.get_instance(user_pk, measurement),
    );

    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;

    // FIXME(max): Querying for the sealed seed using the user_pk in place of
    // the node_pk saves one round trip to the API, but requires joining across
    // four tables. This optimization can decrease boot time.
    match (node_opt, instance_opt) {
        (Some(node), Some(instance)) => {
            ensure!(
                node.node_pk == instance.node_pk,
                "node.node_pk '{}' doesn't match instance.node_pk '{}'",
                node.node_pk,
                instance.node_pk,
            );
            ensure!(
                instance.measurement == measurement,
                "Returned instance measurement '{}' doesn't match \
                 requested measurement '{}'",
                instance.measurement,
                measurement,
            );

            let sealed_seed_id = SealedSeedId {
                node_pk: node.node_pk,
                measurement,
                machine_id,
                min_cpusvn,
            };

            let raw_sealed_data = api
                .get_sealed_seed(sealed_seed_id)
                .await
                .context("Error while fetching sealed seed")?
                .context("Sealed seed wasn't persisted with node & instance")?;

            let sealed_data = Sealed::deserialize(&raw_sealed_data.seed)
                .context("Failed to deserialize sealed seed")?;

            let provisioned_secrets =
                ProvisionedSecrets::unseal(sealed_data)
                    .context("Failed to unseal provisioned secrets")?;

            Ok((node, provisioned_secrets))
        }
        (None, None) => bail!("Enclave version has not been provisioned yet"),
        _ => bail!("Node and instance should have been persisted together"),
    }
}

/// Initializes the ChannelMonitors
async fn channel_monitors(
    persister: &LexePersister,
    keys_manager: LexeKeysManager,
) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
    debug!("reading channel monitors from DB");
    persister
        .read_channel_monitors(keys_manager)
        .await
        .context("Could not read channel monitors")
}

/// Initializes a GossipSync and NetworkGraph
async fn gossip_sync(
    network: Network,
    persister: &LexePersister,
    logger: LexeTracingLogger,
) -> anyhow::Result<(Arc<NetworkGraphType>, Arc<P2PGossipSyncType>)> {
    debug!("initializing gossip sync and network graph");
    let genesis = genesis_block(network.into_inner()).header.block_hash();

    let network_graph = persister
        .read_network_graph(genesis, logger.clone())
        .await
        .context("Could not read network graph")?;
    let network_graph = Arc::new(network_graph);

    let gossip_sync = P2PGossipSync::new(
        Arc::clone(&network_graph),
        None::<Arc<dyn chain::Access + Send + Sync>>,
        logger.clone(),
    );
    let gossip_sync = Arc::new(gossip_sync);

    debug!("gossip sync and network graph done.");
    Ok((network_graph, gossip_sync))
}

/// Sets up a `TcpListener` to listen on 0.0.0.0:<peer_port>, handing off
/// resultant `TcpStream`s for the `PeerManager` to manage
async fn spawn_p2p_listener(
    peer_manager: LexePeerManager,
    peer_port_opt: Option<Port>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> (LxTask<()>, Port) {
    // A value of 0 indicates that the OS will assign a port for us
    // TODO(phlip9): should only listen on internal interface
    let address = format!("0.0.0.0:{}", peer_port_opt.unwrap_or(0));
    let listener = TcpListener::bind(address)
        .await
        .expect("Failed to bind to peer port");
    let peer_port = listener.local_addr().unwrap().port();
    info!("Listening for LN P2P connections on port {}", peer_port);

    let handle = LxTask::spawn(async move {
        let mut child_handles = Vec::with_capacity(1);

        loop {
            tokio::select! {
                accept_res = listener.accept() => {
                    // TcpStream boilerplate
                    let (tcp_stream, _peer_addr) = match accept_res {
                        Ok(ts) => ts,
                        Err(e) => {
                            warn!("Failed to accept connection: {e:#}");
                            continue;
                        }
                    };
                    let tcp_stream = match tcp_stream.into_std() {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Couldn't convert to std TcpStream: {e:#}");
                            continue;
                        }
                    };

                    // Spawn a task to await on the connection
                    let peer_manager_clone = peer_manager.as_arc_inner();
                    let child_handle = LxTask::spawn(async move {
                        // `setup_inbound()` returns a future that completes
                        // when the connection is closed. The main thread calls
                        // peer_manager.disconnect_all_peers() once it receives
                        // a shutdown signal so there is no need to pass in
                        // `shutdown_rx`s here.
                        let connection_closed = lightning_net_tokio::setup_inbound(
                            peer_manager_clone,
                            tcp_stream,
                        );
                        connection_closed.await;
                    });

                    child_handles.push(child_handle);
                }
                _ = shutdown_rx.recv() =>
                    break info!("LN P2P listen task shutting down"),
            }
        }

        // Wait on all child tasks to finish (i.e. all connections close).
        for res in future::join_all(child_handles).await {
            if let Err(e) = res {
                error!("P2P task panicked: {:#}", e);
            }
        }

        info!("LN P2P listen task complete");
    });

    (handle, peer_port)
}

/// Spawns a task that regularly reconnects to the channel peers stored in DB.
fn spawn_p2p_reconnector(
    channel_manager: LexeChannelManager,
    peer_manager: LexePeerManager,
    persister: LexePersister,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> LxTask<()> {
    LxTask::spawn(async move {
        let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

        loop {
            interval.tick().await;
            tokio::select! {
                // Prevents race condition where we initiate a reconnect *after*
                // a shutdown signal was received, causing this task to hang
                biased;
                _ = shutdown_rx.recv() => break info!("P2P reconnector shutting down"),
                _ = interval.tick() => {}
            }

            // NOTE: Repeatedly hitting the DB here doesn't seem strictly
            // necessary (a channel for the channel manager to notify this task
            // of a new peer is sufficient), but it is the simplest solution for
            // now. This can be optimized away if it becomes a problem later.
            let channel_peers = match persister.read_channel_peers().await {
                Ok(cp_vec) => cp_vec,
                Err(e) => {
                    error!("ERROR: Could not read channel peers: {:#}", e);
                    continue;
                }
            };

            // Find all the peers we've been disconnected from
            let p2p_peers = peer_manager.get_peer_node_ids();
            let disconnected_peers: Vec<_> = channel_manager
                .list_channels()
                .iter()
                .map(|chan| chan.counterparty.node_id)
                .filter(|node_id| !p2p_peers.contains(node_id))
                .collect();

            // Match ids
            let mut connect_futs: Vec<_> =
                Vec::with_capacity(disconnected_peers.len());
            for node_id in disconnected_peers {
                for channel_peer in channel_peers.iter() {
                    if channel_peer.pk == node_id {
                        let connect_fut = peer_manager
                            .do_connect_peer(channel_peer.deref().clone());
                        connect_futs.push(connect_fut)
                    }
                }
            }

            // Reconnect
            for res in future::join_all(connect_futs).await {
                if let Err(e) = res {
                    warn!("Couldn't neconnect to channel peer: {:#}", e)
                }
            }
        }
    })
}

/// Spawns a task that that lets the persister make calls to the chain monitor.
/// For now, it simply listens on `channel_monitor_updated_rx` and calls
/// `ChainMonitor::channel_monitor_updated()` with any received values. This is
/// required because (a) the chain monitor cannot be initialized without the
/// persister, therefore (b) the persister cannot hold the chain monitor,
/// therefore there needs to be another means of letting the persister notify
/// the channel manager of events.
pub fn spawn_channel_monitor_updated_task(
    chain_monitor: Arc<ChainMonitorType>,
    mut channel_monitor_updated_rx: mpsc::Receiver<LxChannelMonitorUpdate>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> LxTask<()> {
    debug!("Starting channel_monitor_updated task");
    LxTask::spawn(async move {
        loop {
            tokio::select! {
                Some(update) = channel_monitor_updated_rx.recv() => {
                    if let Err(e) = chain_monitor.channel_monitor_updated(
                        OutPoint::from(update.funding_txo),
                        update.update_id,
                    ) {
                        // ApiError impls Debug but not std::error::Error
                        error!("channel_monitor_updated returned Err: {:?}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("channel_monitor_updated task shutting down");
                    break;
                }
            }
        }
    })
}
