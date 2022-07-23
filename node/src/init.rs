use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::BlockHash;
use common::enclave::{self, Measurement};
use common::rng::Crng;
use common::root_seed::RootSeed;
use lightning::chain;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::routing::gossip::P2PGossipSync;
use lightning_background_processor::BackgroundProcessor;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use secrecy::ExposeSecret;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};

use crate::api::{
    ApiClient, Enclave, Instance, Node, NodeInstanceEnclave, UserPort,
};
use crate::cli::{Network, StartCommand};
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::peer_manager::LexePeerManager;
use crate::lexe::persister::LexePersister;
use crate::lexe::sync::SyncedChainListeners;
use crate::types::{
    ApiClientType, BlockSourceType, BroadcasterType, ChainMonitorType,
    ChannelMonitorType, FeeEstimatorType, GossipSyncType, InvoicePayerType,
    NetworkGraphType, P2PGossipSyncType, PaymentInfoStorageType, Port, UserId,
};
use crate::{api, command, convert};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;

// TODO: Eventually move this into the `lexe` module once init is cleaned up
// TODO: Remove once keys_manager, persister, invoice_payer are read in SGX
#[allow(dead_code)]
pub struct LexeContext {
    args: StartCommand,
    shutdown_tx: broadcast::Sender<()>,

    pub channel_manager: LexeChannelManager,
    pub peer_manager: LexePeerManager,
    keys_manager: LexeKeysManager,
    persister: LexePersister,
    chain_monitor: Arc<ChainMonitorType>,
    pub network_graph: Arc<NetworkGraphType>,
    invoice_payer: Arc<InvoicePayerType>,
    block_source: Arc<BlockSourceType>,
    fee_estimator: Arc<FeeEstimatorType>,
    broadcaster: Arc<BroadcasterType>,
    logger: LexeTracingLogger,

    sync_ctx: Option<SyncContext>,
    run_ctx: Option<RunContext>,
}

/// Variables that only sync() uses, or which sync() requires ownership of
struct SyncContext {
    restarting_node: bool,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    channel_manager_blockhash: BlockHash,
    activity_rx: mpsc::Receiver<()>,
}

/// Variables that only run() uses, or which run() requires ownership of
#[allow(dead_code)] // TODO: Remove once in/outbound payments are read in SGX
struct RunContext {
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
    shutdown_rx: broadcast::Receiver<()>,
    stop_listen_connect: Arc<AtomicBool>,
    background_processor: BackgroundProcessor,
}

impl LexeContext {
    pub async fn init<R: Crng>(
        rng: &mut R,
        args: StartCommand,
    ) -> anyhow::Result<Self> {
        // Initialize the Logger
        let logger = LexeTracingLogger::new();

        // Get user_id, measurement, and HTTP client, used throughout init
        let user_id = args.user_id;
        let measurement = enclave::measurement();
        let api = init_api(&args);

        // Initialize LexeBitcoind, fetch provisioned data
        let (bitcoind_res, provisioned_data_res) = tokio::join!(
            LexeBitcoind::init(args.bitcoind_rpc.clone(), args.network),
            fetch_provisioned_data(api.as_ref(), user_id, measurement),
        );
        let bitcoind =
            bitcoind_res.context("Failed to init bitcoind client")?;
        let provisioned_data =
            provisioned_data_res.context("Failed to fetch provisioned data")?;

        // Build LexeKeysManager from node init data
        let keys_manager = match provisioned_data {
            (Some(node), Some(_i), Some(enclave)) => {
                // TODO(phlip9): actually unseal seed
                let root_seed = RootSeed::try_from(enclave.seed.as_slice())
                    .context("Invalid root seed")?;

                LexeKeysManager::init(rng, &node.public_key, &root_seed)
                    .context("Could not construct keys manager")?
            }
            (None, None, None) => {
                // TODO remove this path once provisioning command works
                provision_new_node(rng, api.as_ref(), user_id, measurement)
                    .await
                    .context("Failed to provision new node")?
            }
            _ => panic!("Node init data should have been persisted atomically"),
        };
        let pubkey = keys_manager.derive_pubkey(rng);
        let instance_id = convert::get_instance_id(&pubkey, &measurement);

        // LexeBitcoind implements BlockSource, FeeEstimator and
        // BroadcasterInterface, and thus serves these functions. A new type
        // alias is defined for each of these in case these functions need to be
        // handled separately later.
        let block_source = bitcoind.clone();
        let fee_estimator = bitcoind.clone();
        let broadcaster = bitcoind.clone();

        // Initialize Persister
        let persister = LexePersister::new(api.clone(), instance_id);

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
        spawn_p2p_listener(
            args.peer_port,
            stop_listen_connect.clone(),
            peer_manager.clone(),
        );

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
            user_id,
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
        let background_processor = BackgroundProcessor::start(
            persister.clone(),
            invoice_payer.clone(),
            chain_monitor.clone(),
            channel_manager.clone(),
            GossipSyncType::P2P(gossip_sync.clone()),
            peer_manager.as_arc_inner(),
            logger.clone(),
            Some(scorer.clone()),
        );

        // Spawn a task to regularly reconnect to channel peers
        spawn_p2p_reconnect_task(
            channel_manager.clone(),
            peer_manager.clone(),
            stop_listen_connect.clone(),
            persister.clone(),
        );

        // Build and return the LexeContext
        let sync_ctx = SyncContext {
            restarting_node,
            channel_manager_blockhash,
            channel_monitors,
            activity_rx,
        };
        let run_ctx = RunContext {
            inbound_payments,
            outbound_payments,
            shutdown_rx,
            stop_listen_connect,
            background_processor,
        };
        let ctx = LexeContext {
            args,
            shutdown_tx,

            channel_manager,
            peer_manager,
            keys_manager,
            persister,
            chain_monitor,
            network_graph,
            invoice_payer,
            block_source,
            fee_estimator,
            broadcaster,
            logger,

            sync_ctx: Some(sync_ctx),
            run_ctx: Some(run_ctx),
        };
        Ok(ctx)
    }

    pub async fn sync(&mut self) -> anyhow::Result<()> {
        let sync_ctx = self.sync_ctx.take().expect("Was set during init");

        // Sync channel manager and channel monitors to chain tip
        let synced_chain_listeners = SyncedChainListeners::init_and_sync(
            self.args.network,
            self.channel_manager.clone(),
            sync_ctx.channel_manager_blockhash,
            sync_ctx.channel_monitors,
            self.block_source.clone(),
            self.broadcaster.clone(),
            self.fee_estimator.clone(),
            self.logger.clone(),
            sync_ctx.restarting_node,
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
            sync_ctx.activity_rx,
            self.shutdown_tx.clone(),
            timer_shutdown_rx,
        );
        tokio::spawn(async move {
            inactivity_timer.start().await;
        });

        Ok(())
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut run_ctx = self.run_ctx.take().expect("Was set during init");

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
                run_ctx.inbound_payments,
                run_ctx.outbound_payments,
                self.persister.clone(),
                self.args.network,
            )
            .await;
            println!("REPL complete.");
        }

        // Pause here and wait for the shutdown signal
        let _ = run_ctx.shutdown_rx.recv().await;

        // ## Shutdown
        println!("Main thread shutting down");

        // Disconnect our peers and stop accepting new connections. This ensures
        // we don't continue updating our channel data after we've
        // stopped the background processor.
        run_ctx.stop_listen_connect.store(true, Ordering::Release);
        self.peer_manager.disconnect_all_peers();

        // Stop the background processor.
        run_ctx.background_processor.stop().unwrap();

        Ok(())
    }
}

/// Constructs a Arc<dyn ApiClient> based on whether we are running in SGX,
/// and whether `args.mock` is set to true
fn init_api(args: &StartCommand) -> ApiClientType {
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
// the provisioned components (Node, Instance, Enclave) were not persisted
// atomically.
/// Fetches previously provisioned data from the API.
async fn fetch_provisioned_data(
    api: &dyn ApiClient,
    user_id: UserId,
    measurement: Measurement,
) -> anyhow::Result<(Option<Node>, Option<Instance>, Option<Enclave>)> {
    println!("Fetching provisioned data");
    let (node_res, instance_res, enclave_res) = tokio::join!(
        api.get_node(user_id),
        api.get_instance(user_id, measurement),
        api.get_enclave(user_id, measurement),
    );
    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;
    let enclave_opt = enclave_res.context("Error while fetching enclave")?;

    Ok((node_opt, instance_opt, enclave_opt))
}

/// A temporary helper to provision a new node when running the start command.
/// Once we have end-to-end provisioning with the client, this function should
/// be removed entirely. TODO: Remove this function
async fn provision_new_node<R: Crng>(
    rng: &mut R,
    api: &dyn ApiClient,
    user_id: UserId,
    measurement: Measurement,
) -> anyhow::Result<LexeKeysManager> {
    // No node exists yet, create a new one
    println!("Generating new seed");

    // TODO Get and use the root seed from provisioning step
    // TODO (sgx): Seal seed under this enclave's pubkey
    let root_seed = RootSeed::from_rng(rng);
    let sealed_seed = root_seed.expose_secret().to_vec();

    // Derive pubkey
    let keys_manager = LexeKeysManager::unchecked_init(rng, &root_seed);
    let node_public_key = keys_manager.derive_pubkey(rng);

    // Build structs for persisting the new node + instance + enclave
    let node = Node {
        public_key: node_public_key,
        user_id,
    };
    let instance_id = convert::get_instance_id(&node_public_key, &measurement);
    let instance = Instance {
        id: instance_id.clone(),
        measurement,
        node_public_key,
    };
    // TODO Actually get the CPU id from within SGX
    let cpu_id = "my_cpu_id";
    let enclave_id = convert::get_enclave_id(instance_id.as_str(), cpu_id);
    let enclave = Enclave {
        id: enclave_id,
        // NOTE: This should be sealed
        seed: sealed_seed,
        instance_id,
    };

    // Persist node, instance, and enclave together in one db txn to
    // ensure that we never end up with a node without a corresponding
    // instance + enclave that can unseal the associated seed
    let node_instance_enclave = NodeInstanceEnclave {
        node,
        instance,
        enclave,
    };
    api.create_node_instance_enclave(node_instance_enclave)
        .await
        .context("Could not atomically create new node + instance + enclave")?;

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

/// Sets up a TcpListener to listen on 0.0.0.0:<peer_port>, handing off
/// resultant `TcpStream`s for the `PeerManager` to manage
fn spawn_p2p_listener(
    peer_port_opt: Option<Port>,
    stop_listen: Arc<AtomicBool>,
    peer_manager: LexePeerManager,
) {
    tokio::spawn(async move {
        // A value of 0 indicates that the OS will assign a port for us
        let address = format!("0.0.0.0:{}", peer_port_opt.unwrap_or(0));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .expect("Failed to bind to peer port");
        let peer_port = listener.local_addr().unwrap().port();
        println!("Listening for P2P connections at port {}", peer_port);
        loop {
            let (tcp_stream, _peer_addr) = listener.accept().await.unwrap();
            let tcp_stream = tcp_stream.into_std().unwrap();
            let peer_manager_clone = peer_manager.as_arc_inner();
            if stop_listen.load(Ordering::Acquire) {
                return;
            }
            tokio::spawn(async move {
                lightning_net_tokio::setup_inbound(
                    peer_manager_clone,
                    tcp_stream,
                )
                .await;
            });
        }
    });
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
                            if channel_peer.pubkey == node_id {
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
