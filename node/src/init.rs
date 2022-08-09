use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::BlockHash;
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use common::api::runner::{Port, UserPort};
use common::api::UserPk;
use common::cli::{Network, StartArgs};
use common::enclave::{
    self, MachineId, Measurement, MinCpusvn, MIN_SGX_CPUSVN,
};
use common::rng::Crng;
use common::root_seed::RootSeed;
use lightning::chain;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::routing::gossip::P2PGossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use secrecy::ExposeSecret;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};

use crate::api::ApiClient;
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::lexe::background_processor::LexeBackgroundProcessor;
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::LexeChannelManager;
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
    args: StartArgs,
    shutdown_tx: broadcast::Sender<()>,
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

    // --- Sync --- //
    restarting_node: bool,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    channel_manager_blockhash: BlockHash,
    activity_rx: mpsc::Receiver<()>,

    // --- Run --- //
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
    shutdown_rx: broadcast::Receiver<()>,
    stop_listen_connect: Arc<AtomicBool>,
}

impl LexeNode {
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: StartArgs,
    ) -> anyhow::Result<Self> {
        // Initialize the Logger
        let logger = LexeTracingLogger::new();

        // Get user_pk, measurement, and HTTP client, used throughout init
        let user_pk = args.user_pk;
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = MIN_SGX_CPUSVN;
        let api = init_api(&args);

        // Initialize LexeBitcoind, fetch provisioned data
        let (bitcoind_res, provisioned_data_res) = tokio::join!(
            LexeBitcoind::init(args.bitcoind_rpc.clone(), args.network),
            fetch_provisioned_data(
                api.as_ref(),
                user_pk,
                measurement,
                machine_id,
                min_cpusvn
            ),
        );
        let bitcoind =
            bitcoind_res.context("Failed to init bitcoind client")?;
        let provisioned_data =
            provisioned_data_res.context("Failed to fetch provisioned data")?;

        // Build LexeKeysManager from node init data
        let keys_manager = match provisioned_data {
            Some((node, _instance, sealed_seed)) => {
                // TODO(phlip9): actually unseal seed
                let root_seed = RootSeed::try_from(sealed_seed.seed.as_slice())
                    .context("Invalid root seed")?;

                LexeKeysManager::init(rng, &node.node_pk, &root_seed)
                    .context("Could not construct keys manager")?
            }
            None => {
                // TODO remove this path once provisioning command works
                provision_new_node(
                    rng,
                    api.as_ref(),
                    user_pk,
                    measurement,
                    machine_id,
                    min_cpusvn,
                )
                .await
                .context("Failed to provision new node")?
            }
        };
        let node_pk = keys_manager.derive_pk(rng);

        // LexeBitcoind implements BlockSource, FeeEstimator and
        // BroadcasterInterface, and thus serves these functions. It also
        // serves as the wallet for now. A type alias is defined for each of
        // these in case they need to be split apart later.
        let wallet = bitcoind.clone();
        let block_source = bitcoind.clone();
        let fee_estimator = bitcoind.clone();
        let broadcaster = bitcoind.clone();

        // Initialize Persister
        let persister = LexePersister::new(api.clone(), node_pk, measurement);

        // Initialize the ChainMonitor
        let chain_monitor = Arc::new(ChainMonitor::new(
            None,
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            persister.clone(),
        ));

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

        // Init Tokio channels
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (shutdown_tx, shutdown_rx) =
            broadcast::channel(DEFAULT_CHANNEL_SIZE);

        // Start warp at the given port, or bind to an ephemeral port if not
        // given
        let routes = command::server::routes(
            channel_manager.clone(),
            peer_manager.clone(),
            network_graph.clone(),
            activity_tx,
            shutdown_tx.clone(),
        );
        let (addr, server_fut) = warp::serve(routes)
            // A value of 0 indicates that the OS will assign a port for us
            .try_bind_ephemeral(([127, 0, 0, 1], args.warp_port.unwrap_or(0)))
            .context("Failed to bind warp")?;
        let warp_port = addr.port();
        println!("Serving warp at port {}", warp_port);
        tokio::spawn(async move {
            server_fut.await;
        });

        // Let the runner know that we're ready
        println!("Node is ready to accept commands; notifying runner");
        let user_port = UserPort {
            user_pk,
            port: warp_port,
        };
        api.notify_runner(user_port)
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
        let bgp_shutdown_rx = shutdown_tx.subscribe();
        let _bgp_handle = LexeBackgroundProcessor::start(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            invoice_payer.clone(),
            gossip_sync.clone(),
            scorer.clone(),
            shutdown_tx.clone(),
            bgp_shutdown_rx,
        );

        // Spawn a task to regularly reconnect to channel peers
        spawn_p2p_reconnect_task(
            channel_manager.clone(),
            peer_manager.clone(),
            stop_listen_connect.clone(),
            persister.clone(),
        );

        // Build and return the LexeNode
        let node = LexeNode {
            // General
            args,
            shutdown_tx,
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

            // Sync
            restarting_node,
            channel_manager_blockhash,
            channel_monitors,
            activity_rx,

            // Run
            inbound_payments,
            outbound_payments,
            shutdown_rx,
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
        println!("Starting inactivity timer");
        let timer_shutdown_rx = self.shutdown_tx.subscribe();
        let mut inactivity_timer = InactivityTimer::new(
            self.args.shutdown_after_sync_if_no_activity,
            self.args.inactivity_timer_sec,
            self.activity_rx,
            self.shutdown_tx.clone(),
            timer_shutdown_rx,
        );
        tokio::spawn(async move {
            inactivity_timer.start().await;
        });

        // --- Run --- //

        // Start the REPL if it was specified to start in the CLI args.
        #[cfg(not(target_env = "sgx"))]
        if self.args.repl {
            println!("Starting REPL");
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
            println!("REPL complete.");
        }

        // Pause here and wait for the shutdown signal
        let _ = self.shutdown_rx.recv().await;

        // --- Shutdown --- //
        println!("Main thread shutting down");

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
fn init_api(args: &StartArgs) -> ApiClientType {
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

// TODO: After the provision flow has been implemented, this function should be
// changed to error if any of these endpoints returned None - indicating that
// the provisioned components (Node, Instance, SealedSeed) were not persisted
// atomically.
/// Fetches previously provisioned data from the API.
async fn fetch_provisioned_data(
    api: &dyn ApiClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
    min_cpusvn: MinCpusvn,
) -> anyhow::Result<Option<(Node, Instance, SealedSeed)>> {
    println!("Fetching provisioned data");
    let (node_res, instance_res) = tokio::join!(
        api.get_node(user_pk),
        api.get_instance(user_pk, measurement),
    );

    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;

    // FIXME(max): It is faster to query for the sealed seed using the user_pk
    // in place of the node_pk, but that requires joining across four tables.
    // This is a quick optimization that can be done to decrease boot time.
    let tuple_opt = match (node_opt, instance_opt) {
        (Some(node), Some(instance)) => {
            let sealed_seed_id = SealedSeedId {
                node_pk: node.node_pk,
                measurement,
                machine_id,
                min_cpusvn,
            };

            let sealed_seed = api
                .get_sealed_seed(sealed_seed_id)
                .await
                .context("Error while fetching sealed seed")?
                .expect("Sealed seed wasn't persisted with node & instance");

            Some((node, instance, sealed_seed))
        }
        (None, None) => None,
        _ => panic!("Node and instance should have been persisted together"),
    };

    Ok(tuple_opt)
}

/// A temporary helper to provision a new node when running the start command.
/// Once we have end-to-end provisioning with the client, this function should
/// be removed entirely. TODO: Remove this function
async fn provision_new_node<R: Crng>(
    rng: &mut R,
    api: &dyn ApiClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
    min_cpusvn: MinCpusvn,
) -> anyhow::Result<LexeKeysManager> {
    // No node exists yet, create a new one
    println!("Generating new seed");

    // TODO Get and use the root seed from provisioning step
    // TODO (sgx): Seal seed under this enclave's public key
    let root_seed = RootSeed::from_rng(rng);
    let seed = root_seed.expose_secret().to_vec();

    // Derive node pk
    let keys_manager = LexeKeysManager::insecure_init(rng, &root_seed);
    let node_pk = keys_manager.derive_pk(rng);

    // Build structs for persisting the new node + instance + seed
    let node = Node { node_pk, user_pk };
    let instance = Instance {
        measurement,
        node_pk,
    };
    let sealed_seed =
        SealedSeed::new(node_pk, measurement, machine_id, min_cpusvn, seed);

    // Persist node, instance, and seed together in one db txn
    let node_instance_seed = NodeInstanceSeed {
        node,
        instance,
        sealed_seed,
    };
    api.create_node_instance_seed(node_instance_seed)
        .await
        .context("Could not atomically create new node + instance + seed")?;

    Ok(keys_manager)
}

/// Initializes the ChannelMonitors
async fn channel_monitors(
    persister: &LexePersister,
    keys_manager: LexeKeysManager,
) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
    println!("Reading channel monitors from DB");
    let result = persister
        .read_channel_monitors(keys_manager)
        .await
        .context("Could not read channel monitors");

    println!("    channel monitors done.");
    result
}

/// Initializes a GossipSync and NetworkGraph
async fn gossip_sync(
    network: Network,
    persister: &LexePersister,
    logger: LexeTracingLogger,
) -> anyhow::Result<(Arc<NetworkGraphType>, Arc<P2PGossipSyncType>)> {
    println!("Initializing gossip sync and network graph");
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

    println!("    gossip sync and network graph done.");
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
    let address = format!("0.0.0.0:{}", peer_port_opt.unwrap_or(0));
    let listener = TcpListener::bind(address)
        .await
        .expect("Failed to bind to peer port");
    let peer_port = listener.local_addr().unwrap().port();
    println!("Listening for P2P connections at port {}", peer_port);
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
) {
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
                    println!("ERROR: Could not read channel peers: {}", e)
                }
            }
        }
    });
}
