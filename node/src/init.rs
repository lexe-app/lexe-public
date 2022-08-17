use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
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
use lightning::chain;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::chain::transaction::OutPoint;
use lightning::routing::gossip::P2PGossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument};

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
    ChannelMonitorType, FeeEstimatorType, InvoicePayerType, NetworkGraphType,
    P2PGossipSyncType, PaymentInfoStorageType, WalletType,
};
use crate::{api, command};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;

// TODO: Eventually move this into the `lexe` module once init is cleaned up
// TODO: Remove once keys_manager, persister, invoice_payer are read in SGX
#[allow(dead_code)]
pub struct LexeNode {
    // --- General --- //
    args: RunArgs,
    pub peer_port: Port,

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
    main_thread_shutdown_rx: broadcast::Receiver<()>,
    stop_listen_connect: Arc<AtomicBool>,
}

impl LexeNode {
    #[instrument(skip_all)]
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: RunArgs,
    ) -> anyhow::Result<Self> {
        // Initialize the Logger
        let logger = LexeTracingLogger::new();
        info!(%args.user_pk, "initializing node");

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

        // Spawn task to refresh feerates
        // TODO(max): Handle the handle
        let _refresh_fees_handle = bitcoind.spawn_refresh_fees_task();

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
        let shutdown = ShutdownChannel::new();
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
        // TODO(max): Handle the handle
        let _channel_monitor_updated_handle =
            spawn_channel_monitor_updated_task(
                chain_monitor.clone(),
                channel_monitor_updated_rx,
                shutdown.rx.channel_monitor_updated,
            );

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
        let stop_listen_connect = Arc::new(AtomicBool::new(false));
        let peer_port = spawn_p2p_listener(
            args.peer_port,
            stop_listen_connect.clone(),
            peer_manager.clone(),
        )
        .await;

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
        // TODO(max): Handle the handle
        let _owner_service_handle = tokio::spawn(async move {
            owner_service_fut.await;
        });

        // TODO(phlip9): authenticate host<->node
        // Start warp service for host
        let host_routes = command::server::host_routes(shutdown.tx.host_routes);
        let (host_addr, host_service_fut) = warp::serve(host_routes)
            // A value of 0 indicates that the OS will assign a port for us
            .try_bind_ephemeral(([127, 0, 0, 1], args.host_port.unwrap_or(0)))
            .context("Failed to bind warp")?;
        let host_port = host_addr.port();
        info!("Host service listening on port {}", host_port);
        // TODO(max): Handle the handle
        let _host_service_handle = tokio::spawn(async move {
            host_service_fut.await;
        });

        // Let the runner know that we're ready
        info!("Node is ready to accept commands; notifying runner");
        let user_ports =
            UserPorts::new_run(user_pk, owner_port, host_port, peer_port);
        api.notify_runner(user_ports)
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
            Handle::current(),
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
        // TODO(max): Handle the handle
        let _bgp_handle = LexeBackgroundProcessor::start(
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

        // Construct (but don't start) the inactivity timer
        let inactivity_timer = InactivityTimer::new(
            args.shutdown_after_sync_if_no_activity,
            args.inactivity_timer_sec,
            activity_rx,
            shutdown.tx.inactivity_timer,
            shutdown.rx.inactivity_timer,
        );

        // Spawn a task to regularly reconnect to channel peers
        // TODO(max): Handle the handle
        let _reconnect_handle = spawn_p2p_reconnect_task(
            channel_manager.clone(),
            peer_manager.clone(),
            stop_listen_connect.clone(),
            persister.clone(),
        );

        // Build and return the LexeNode
        let main_thread_shutdown_rx = shutdown.rx.main_thread;
        let node = LexeNode {
            // General
            args,
            peer_port,

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
            main_thread_shutdown_rx,
            stop_listen_connect,
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
        synced_chain_listeners
            .feed_chain_monitor_and_spawn_spv(self.chain_monitor.clone())
            .context("Error wrapping up sync")?;

        // Sync is complete; start the inactivity timer.
        debug!("Starting inactivity timer");
        tokio::spawn(async move {
            self.inactivity_timer.start().await;
        });

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
        let _ = self.main_thread_shutdown_rx.recv().await;

        // --- Shutdown --- //
        info!("Main thread shutting down");

        // Disconnect from peers and stop accepting new connections. This
        // ensures we don't continue updating our channel data after we've
        // stopped the background processor.
        self.stop_listen_connect.store(true, Ordering::Release);
        self.peer_manager.disconnect_all_peers();

        Ok(())
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

    // FIXME(max): It is faster to query for the sealed seed using the user_pk
    // in place of the node_pk, but that requires joining across four tables.
    // This is a quick optimization that can be done to decrease boot time.
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
    main_thread: broadcast::Receiver<()>,
    inactivity_timer: broadcast::Receiver<()>,
    background_processor: broadcast::Receiver<()>,
    channel_monitor_updated: broadcast::Receiver<()>,
}

impl ShutdownChannel {
    /// Initializes the `shutdown_tx` / `shutdown_rx` channel senders and
    /// receivers. These are initialized as structs to ensure that *all* calls
    /// to [`subscribe`] are complete before any values are sent.
    ///
    /// [`subscribe`]: broadcast::Sender::subscribe
    fn new() -> Self {
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
        let main_thread = shutdown_tx.subscribe();
        let inactivity_timer = shutdown_tx.subscribe();
        let background_processor = shutdown_tx.subscribe();
        let channel_monitor_updated = shutdown_tx.subscribe();
        let rx = ShutdownRx {
            main_thread,
            inactivity_timer,
            background_processor,
            channel_monitor_updated,
        };

        Self { tx, rx }
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
    peer_port_opt: Option<Port>,
    stop_listen: Arc<AtomicBool>,
    peer_manager: LexePeerManager,
) -> Port {
    // A value of 0 indicates that the OS will assign a port for us
    // TODO(phlip9): should only listen on internal interface
    let address = format!("0.0.0.0:{}", peer_port_opt.unwrap_or(0));
    let listener = TcpListener::bind(address)
        .await
        .expect("Failed to bind to peer port");
    let peer_port = listener.local_addr().unwrap().port();
    info!("lightning peer listening on port {}", peer_port);
    tokio::spawn(async move {
        loop {
            let (tcp_stream, _peer_addr) = listener.accept().await.unwrap();
            let tcp_stream = tcp_stream.into_std().unwrap();
            let peer_manager_clone = peer_manager.as_arc_inner();
            if stop_listen.load(Ordering::Acquire) {
                return;
            }
            tokio::spawn(async move {
                // `setup_inbound()` returns a future that completes when the
                // connection is closed.
                let connection_closed = lightning_net_tokio::setup_inbound(
                    peer_manager_clone,
                    tcp_stream,
                );
                connection_closed.await;
            });
        }
    });
    peer_port
}

/// Spawns a task that regularly reconnects to the channel peers stored in DB.
fn spawn_p2p_reconnect_task(
    channel_manager: LexeChannelManager,
    peer_manager: LexePeerManager,
    stop_listen_connect: Arc<AtomicBool>,
    persister: LexePersister,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            match persister.read_channel_peers().await {
                Ok(cp_vec) => {
                    let peers = peer_manager.get_peer_node_ids();
                    for node_id in channel_manager
                        .list_channels()
                        .iter()
                        .map(|chan| chan.counterparty.node_id)
                        .filter(|id| !peers.contains(id))
                    {
                        if stop_listen_connect.load(Ordering::Acquire) {
                            return;
                        }
                        for channel_peer in cp_vec.iter() {
                            if channel_peer.pk == node_id {
                                let _ = peer_manager
                                    .do_connect_peer(
                                        channel_peer.deref().clone(),
                                    )
                                    .await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("could not read channel peers: {}", e)
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
) -> JoinHandle<()> {
    info!("Starting channel_monitor_updated task");
    tokio::spawn(async move {
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
