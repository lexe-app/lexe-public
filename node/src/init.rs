use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::BlockHash;
use common::rng::Crng;
use common::root_seed::RootSeed;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::chain::transaction::OutPoint;
use lightning::chain::{self, BestBlock, Watch};
use lightning::ln::channelmanager;
use lightning::ln::channelmanager::ChainParameters;
use lightning::routing::gossip::P2PGossipSync;
use lightning::util::config::UserConfig;
use lightning_background_processor::BackgroundProcessor;
use lightning_block_sync::poll::{self, ValidatedBlockHeader};
use lightning_block_sync::{init as blocksyncinit, SpvClient, UnboundedCache};
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use secrecy::ExposeSecret;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};

use crate::api::{
    ApiClient, Enclave, Instance, Node, NodeInstanceEnclave, UserPort,
};
use crate::bitcoind_client::BitcoindClient;
use crate::cli::StartCommand;
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::keys_manager::LexeKeysManager;
use crate::logger::LdkTracingLogger;
use crate::peer_manager::{self, LexePeerManager};
use crate::persister::LexePersister;
use crate::types::{
    BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorListenerType, ChannelMonitorType, FeeEstimatorType,
    GossipSyncType, InvoicePayerType, Network, NetworkGraphType,
    P2PGossipSyncType, PaymentInfoStorageType, Port, UserId,
};
use crate::{command, convert, repl};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;

pub async fn start_ldk<R: Crng>(
    rng: &mut R,
    args: StartCommand,
) -> anyhow::Result<()> {
    // Initialize the Logger
    let logger = Arc::new(LdkTracingLogger {});

    // Get user_id, measurement, and HTTP client, used throughout init
    let user_id = args.user_id;
    // TODO(sgx) Insert this enclave's measurement
    let measurement = String::from("default");
    let api = ApiClient::new(args.backend_url.clone(), args.runner_url.clone());

    // Initialize BitcoindClient, fetch provisioned data
    let (bitcoind_client_res, provisioned_data_res) = tokio::join!(
        BitcoindClient::init(args.bitcoind_rpc.clone(), args.network),
        fetch_provisioned_data(&api, user_id, &measurement),
    );
    let bitcoind_client =
        bitcoind_client_res.context("Failed to init bitcoind client")?;
    let provisioned_data =
        provisioned_data_res.context("Failed to fetch provisioned data")?;

    // Build LexeKeysManager from node init data
    let keys_manager = match provisioned_data {
        (Some(node), Some(_i), Some(enclave)) => {
            LexeKeysManager::init(rng, node.public_key, enclave.seed)
                .context("Could not construct keys manager")?
        }
        (None, None, None) => {
            // TODO remove this path once provisioning command works
            provision_new_node(rng, &api, user_id, &measurement)
                .await
                .context("Failed to provision new node")?
        }
        _ => panic!("Node init data should have been persisted atomically"),
    };
    let keys_manager = Arc::new(keys_manager);
    let pubkey = keys_manager.derive_pubkey(rng);
    let instance_id = convert::get_instance_id(&pubkey, &measurement);

    // BitcoindClient implements FeeEstimator and BroadcasterInterface and thus
    // serves these functions. This can be customized later.
    let fee_estimator = bitcoind_client.clone();
    let broadcaster = bitcoind_client.clone();

    // Initialize Persister
    let persister = Arc::new(LexePersister::new(api.clone(), instance_id));

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
        channel_monitors(persister.as_ref(), keys_manager.clone()),
        gossip_sync(args.network, &persister, logger.clone())
    );
    let mut channel_monitors =
        channel_monitors_res.context("Could not read channel monitors")?;
    let (network_graph, gossip_sync) =
        gossip_sync_res.context("Could not initialize gossip sync")?;

    // Initialize the ChannelManager and ProbabilisticScorer
    let mut restarting_node = true;
    let (channel_manager_res, scorer_res) = tokio::join!(
        channel_manager(
            &args,
            persister.as_ref(),
            bitcoind_client.as_ref(),
            &mut restarting_node,
            &mut channel_monitors,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            logger.clone(),
        ),
        persister
            .read_probabilistic_scorer(network_graph.clone(), logger.clone()),
    );
    let (channel_manager_blockhash, channel_manager) =
        channel_manager_res.context("Could not init ChannelManager")?;
    let scorer = scorer_res.context("Could not read probabilistic scorer")?;
    let scorer = Arc::new(Mutex::new(scorer));

    // Initialize PeerManager
    let peer_manager = LexePeerManager::init(
        rng,
        keys_manager.as_ref(),
        channel_manager.clone(),
        gossip_sync.clone(),
        logger.clone(),
    );
    let peer_manager = Arc::new(peer_manager);

    // Set up listening for inbound P2P connections
    let stop_listen_connect = Arc::new(AtomicBool::new(false));
    spawn_p2p_listener(
        args.peer_port,
        stop_listen_connect.clone(),
        peer_manager.clone(),
    );

    // Init Tokio channels
    let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
    let (shutdown_tx, mut shutdown_rx) =
        broadcast::channel(DEFAULT_CHANNEL_SIZE);

    // Start warp at the given port, or bind to an ephemeral port if not given
    let routes = command::server::routes(
        channel_manager.clone(),
        peer_manager.clone(),
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
        bitcoind_client.clone(),
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
        Arc::clone(&persister),
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

    // ## Sync

    // Sync channel_monitors and ChannelManager to chain tip
    let mut blockheader_cache = UnboundedCache::new();
    let (chain_listener_channel_monitors, chain_tip) = if restarting_node {
        sync_chain_listeners(
            &args,
            &channel_manager,
            bitcoind_client.as_ref(),
            broadcaster.clone(),
            fee_estimator.clone(),
            logger.clone(),
            channel_manager_blockhash,
            channel_monitors,
            &mut blockheader_cache,
        )
        .await
        .context("Could not sync channel listeners")?
    } else {
        let chain_tip = blocksyncinit::validate_best_block_header(
            &mut bitcoind_client.deref(),
        )
        .await
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not validate best block header")?;

        (Vec::new(), chain_tip)
    };

    // Give channel_monitors to ChainMonitor
    for cmcl in chain_listener_channel_monitors {
        let channel_monitor = cmcl.channel_monitor_listener.0;
        let funding_outpoint = cmcl.funding_outpoint;
        chain_monitor
            .watch_channel(funding_outpoint, channel_monitor)
            .map_err(|e| anyhow!("{:?}", e))
            .context("Could not watch channel")?;
    }

    // Set up SPV client
    spawn_spv_client(
        args.network,
        chain_tip,
        blockheader_cache,
        channel_manager.clone(),
        chain_monitor.clone(),
        bitcoind_client.clone(),
    );

    // ## Ready

    // Start the REPL if it was specified to start in the CLI args.
    if args.repl {
        println!("Starting REPL");
        repl::poll_for_user_input(
            Arc::clone(&invoice_payer),
            Arc::clone(&peer_manager),
            Arc::clone(&channel_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&network_graph),
            inbound_payments,
            outbound_payments,
            persister.clone(),
            args.network,
        )
        .await;
        println!("REPL complete.");
    }

    // Start the inactivity timer.
    println!("Starting inactivity timer");
    let timer_shutdown_rx = shutdown_tx.subscribe();
    let mut inactivity_timer = InactivityTimer::new(
        args.shutdown_after_sync_if_no_activity,
        args.inactivity_timer_sec,
        activity_rx,
        shutdown_tx,
        timer_shutdown_rx,
    );
    tokio::spawn(async move {
        inactivity_timer.start().await;
    });

    // Pause here and wait for the shutdown signal
    let _ = shutdown_rx.recv().await;

    // ## Shutdown
    println!("Main thread shutting down");

    // Disconnect our peers and stop accepting new connections. This ensures we
    // don't continue updating our channel data after we've stopped the
    // background processor.
    stop_listen_connect.store(true, Ordering::Release);
    peer_manager.disconnect_all_peers();

    // Stop the background processor.
    background_processor.stop().unwrap();

    Ok(())
}

// TODO: After the provision flow has been implemented, this function should be
// changed to error if any of these endpoints returned None - indicating that
// the provisioned components (Node, Instance, Enclave) were not persisted
// atomically.
/// Fetches previously provisioned data from the API.
#[cfg(not(test))]
async fn fetch_provisioned_data(
    api: &ApiClient,
    user_id: UserId,
    measurement: &str,
) -> anyhow::Result<(Option<Node>, Option<Instance>, Option<Enclave>)> {
    println!("Fetching provisioned data");
    let (node_res, instance_res, enclave_res) = tokio::join!(
        api.get_node(user_id),
        api.get_instance(user_id, measurement.to_owned()),
        api.get_enclave(user_id, measurement.to_owned()),
    );
    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;
    let enclave_opt = enclave_res.context("Error while fetching enclave")?;

    Ok((node_opt, instance_opt, enclave_opt))
}

/// Returns dummy provisioned data for use in tests.
#[cfg(test)]
async fn fetch_provisioned_data(
    _api: &ApiClient,
    _user_id: UserId,
    _measurement: &str,
) -> anyhow::Result<(Option<Node>, Option<Instance>, Option<Enclave>)> {
    use crate::command::test;

    let node = Node {
        public_key: test::PUBKEY.into(),
        user_id: test::USER_ID.into(),
    };

    let instance = Instance {
        id: test::instance_id(),
        measurement: test::MEASUREMENT.into(),
        node_public_key: test::PUBKEY.into(),
    };

    let enclave = Enclave {
        id: test::enclave_id(),
        seed: test::seed(),
        instance_id: test::instance_id(),
    };

    let node_opt = Some(node);
    let instance_opt = Some(instance);
    let enclave_opt = Some(enclave);

    Ok((node_opt, instance_opt, enclave_opt))
}

/// A temporary helper to provision a new node when running the start command.
/// Once we have end-to-end provisioning with the client, this function should
/// be removed entirely. TODO: Remove this function
async fn provision_new_node<R: Crng>(
    rng: &mut R,
    api: &ApiClient,
    user_id: UserId,
    measurement: &str,
) -> anyhow::Result<LexeKeysManager> {
    // No node exists yet, create a new one
    println!("Generating new seed");

    // TODO Get and use the root seed from provisioning step
    // TODO (sgx): Seal seed under this enclave's pubkey
    let root_seed = RootSeed::from_rng(rng);
    let sealed_seed = root_seed.expose_secret().to_vec();

    // Derive pubkey
    let keys_manager = LexeKeysManager::unchecked_init(rng, root_seed);
    let pubkey = keys_manager.derive_pubkey(rng);
    let pubkey_hex = convert::pubkey_to_hex(&pubkey);

    // Build structs for persisting the new node + instance + enclave
    let node = Node {
        public_key: pubkey_hex.clone(),
        user_id,
    };
    let instance_id = convert::get_instance_id(&pubkey, measurement);
    let instance = Instance {
        id: instance_id.clone(),
        measurement: measurement.to_owned(),
        node_public_key: pubkey_hex,
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
    keys_manager: Arc<LexeKeysManager>,
) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
    println!("Reading channel monitors from DB");
    let result = persister
        .read_channel_monitors(keys_manager)
        .await
        .context("Could not read channel monitors");

    println!("    channel monitors done.");
    result
}

/// Initializes the ChannelManager
#[allow(clippy::too_many_arguments)]
async fn channel_manager(
    args: &StartCommand,
    persister: &LexePersister,
    bitcoind_client: &BitcoindClient,
    restarting_node: &mut bool,
    channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
    keys_manager: Arc<LexeKeysManager>,
    fee_estimator: Arc<FeeEstimatorType>,
    chain_monitor: Arc<ChainMonitorType>,
    broadcaster: Arc<BroadcasterType>,
    logger: Arc<LdkTracingLogger>,
) -> anyhow::Result<(BlockHash, Arc<ChannelManagerType>)> {
    println!("Initializing the channel manager");
    let mut user_config = UserConfig::default();
    user_config
        .peer_channel_config_limits
        .force_announced_channel_preference = false;
    let channel_manager_opt = persister
        .read_channel_manager(
            channel_monitors,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            logger.clone(),
            user_config,
        )
        .await
        .context("Could not read ChannelManager from DB")?;
    let (channel_manager_blockhash, channel_manager) = match channel_manager_opt
    {
        Some((blockhash, mgr)) => (blockhash, mgr),
        None => {
            // We're starting a fresh node.
            *restarting_node = false;
            let getinfo_resp = bitcoind_client.get_blockchain_info().await;

            let chain_params = ChainParameters {
                network: args.network.into_inner(),
                best_block: BestBlock::new(
                    getinfo_resp.latest_blockhash,
                    getinfo_resp.latest_height as u32,
                ),
            };
            let fresh_channel_manager = channelmanager::ChannelManager::new(
                fee_estimator,
                chain_monitor,
                broadcaster,
                logger,
                keys_manager,
                user_config,
                chain_params,
            );
            (getinfo_resp.latest_blockhash, fresh_channel_manager)
        }
    };
    let channel_manager = Arc::new(channel_manager);

    println!("    channel manager done.");
    Ok((channel_manager_blockhash, channel_manager))
}

struct ChannelMonitorChainListener {
    channel_monitor_blockhash: BlockHash,
    channel_monitor_listener: ChannelMonitorListenerType,
    funding_outpoint: OutPoint,
}

/// Syncs the channel monitors and ChannelManager to the chain tip
#[allow(clippy::too_many_arguments)]
async fn sync_chain_listeners(
    args: &StartCommand,
    channel_manager: &ChannelManagerType,
    bitcoind_client: &BitcoindClient,
    broadcaster: Arc<BroadcasterType>,
    fee_estimator: Arc<FeeEstimatorType>,
    logger: Arc<LdkTracingLogger>,
    channel_manager_blockhash: BlockHash,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    blockheader_cache: &mut HashMap<BlockHash, ValidatedBlockHeader>,
) -> anyhow::Result<(Vec<ChannelMonitorChainListener>, ValidatedBlockHeader)> {
    println!("Syncing chain listeners");
    let mut chain_listener_channel_monitors = Vec::new();

    let mut chain_listeners = vec![(
        channel_manager_blockhash,
        channel_manager as &dyn chain::Listen,
    )];

    for (channel_monitor_blockhash, channel_monitor) in channel_monitors {
        let (funding_outpoint, _script) = channel_monitor.get_funding_txo();
        let cmcl = ChannelMonitorChainListener {
            channel_monitor_blockhash,
            channel_monitor_listener: (
                channel_monitor,
                broadcaster.clone(),
                fee_estimator.clone(),
                logger.clone(),
            ),
            funding_outpoint,
        };
        chain_listener_channel_monitors.push(cmcl);
    }

    for monitor_listener_info in chain_listener_channel_monitors.iter_mut() {
        chain_listeners.push((
            monitor_listener_info.channel_monitor_blockhash,
            &monitor_listener_info.channel_monitor_listener
                as &dyn chain::Listen,
        ));
    }

    let chain_tip = blocksyncinit::synchronize_listeners(
        &bitcoind_client,
        args.network.into_inner(),
        blockheader_cache,
        chain_listeners,
    )
    .await
    .map_err(|e| anyhow!(e.into_inner()))
    .context("Could not synchronize chain listeners")?;

    println!("    chain listener sync done.");
    Ok((chain_listener_channel_monitors, chain_tip))
}

/// Initializes a GossipSync and NetworkGraph
async fn gossip_sync(
    network: Network,
    persister: &Arc<LexePersister>,
    logger: Arc<LdkTracingLogger>,
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
    peer_manager: Arc<LexePeerManager>,
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
    channel_manager: Arc<ChannelManagerType>,
    peer_manager: Arc<LexePeerManager>,
    stop_listen_connect: Arc<AtomicBool>,
    persister: Arc<LexePersister>,
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
                        for (pubkey, peer_addr) in cp_vec.iter() {
                            if *pubkey == node_id {
                                let _ = peer_manager::do_connect_peer(
                                    *pubkey,
                                    *peer_addr,
                                    peer_manager.clone(),
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

/// Sets up an SpvClient to continuously poll the block source for new blocks.
fn spawn_spv_client(
    network: Network,
    chain_tip: ValidatedBlockHeader,
    mut blockheader_cache: HashMap<BlockHash, ValidatedBlockHeader>,
    channel_manager: Arc<ChannelManagerType>,
    chain_monitor: Arc<ChainMonitorType>,
    bitcoind_block_source: Arc<BitcoindClient>,
) {
    let channel_manager_clone = channel_manager.clone(); // TODO remove
    let chain_monitor_listener = chain_monitor.clone(); // TODO remove
    tokio::spawn(async move {
        let mut derefed = bitcoind_block_source.deref();
        let chain_poller =
            poll::ChainPoller::new(&mut derefed, network.into_inner());
        let chain_listener = (chain_monitor_listener, channel_manager_clone);
        let mut spv_client = SpvClient::new(
            chain_tip,
            chain_poller,
            &mut blockheader_cache,
            &chain_listener,
        );
        loop {
            spv_client.poll_best_tip().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
