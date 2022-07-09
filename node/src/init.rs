use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, ensure, Context};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::network::constants::Network;
use bitcoin::secp256k1::PublicKey;
use bitcoin::BlockHash;
use lightning::chain::keysinterface::{KeysInterface, KeysManager, Recipient};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{self, chainmonitor, BestBlock, Watch};
use lightning::ln::channelmanager;
use lightning::ln::channelmanager::ChainParameters;
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::routing::gossip::P2PGossipSync;
use lightning::util::config::UserConfig;
use lightning_background_processor::BackgroundProcessor;
use lightning_block_sync::poll::{self, ValidatedBlockHeader};
use lightning_block_sync::{init as blocksyncinit, SpvClient, UnboundedCache};
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use ring::rand::{self, SecureRandom};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc};

use crate::api::{
    self, Enclave, Instance, Node, NodeInstanceEnclave, UserPort,
};
use crate::bitcoind_client::BitcoindClient;
use crate::cli::StartCommand;
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::logger::LdkTracingLogger;
use crate::persister::PostgresPersister;
use crate::types::{
    BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorListenerType, ChannelMonitorType, FeeEstimatorType,
    GossipSyncType, InvoicePayerType, NetworkGraphType, P2PGossipSyncType,
    PaymentInfoStorageType, PeerManagerType, Port, UserId,
};
use crate::{command_server, convert, peer, repl};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;

pub async fn start_ldk(args: StartCommand) -> anyhow::Result<()> {
    let network = args.network.into_inner();

    // Initialize the Logger
    let logger = Arc::new(LdkTracingLogger {});
    let rng = rand::SystemRandom::new();

    // Get user_id, measurement, and HTTP client, used throughout init
    let user_id = args.user_id;
    // TODO(sgx) Insert this enclave's measurement
    let measurement = String::from("default");
    let client = reqwest::Client::new();

    // Initialize BitcoindClient and KeysManager
    let (bitcoind_client_res, keys_manager_res) = tokio::join!(
        bitcoind_client(&args),
        keys_manager(&rng, &client, user_id, &measurement),
    );
    let bitcoind_client =
        bitcoind_client_res.context("Failed to init bitcoind client")?;
    let (pubkey, keys_manager) =
        keys_manager_res.context("Could not init KeysManager")?;

    // BitcoindClient implements FeeEstimator and BroadcasterInterface and thus
    // serves these functions. This can be customized later.
    let fee_estimator = bitcoind_client.clone();
    let broadcaster = bitcoind_client.clone();

    // Initialize Persister
    let persister =
        Arc::new(PostgresPersister::new(&client, &pubkey, &measurement));

    // Initialize the ChainMonitor
    let chain_monitor = Arc::new(chainmonitor::ChainMonitor::new(
        None,
        broadcaster.clone(),
        logger.clone(),
        fee_estimator.clone(),
        persister.clone(),
    ));

    // Read the `ChannelMonitor`s and initialize the `P2PGossipSync`
    let (channel_monitors_res, gossip_sync_res) = tokio::join!(
        channel_monitors(persister.as_ref(), keys_manager.clone()),
        gossip_sync(&args, &persister, logger.clone())
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
    let peer_manager = peer_manager(
        &rng,
        keys_manager.as_ref(),
        channel_manager.clone(),
        gossip_sync.clone(),
        logger.clone(),
    )
    .context("Could not initialize peer manager")?;

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

    // Start warp at the given port
    let routes = command_server::routes(
        channel_manager.clone(),
        peer_manager.clone(),
        activity_tx,
        shutdown_tx.clone(),
    );
    tokio::spawn(async move {
        println!("Serving warp at port {}", args.warp_port);
        warp::serve(routes)
            .run(([127, 0, 0, 1], args.warp_port))
            .await;
    });

    // Let the runner know that we're ready
    let user_port = UserPort {
        user_id,
        port: args.warp_port,
    };
    println!("Node is ready to accept commands; notifying runner");
    api::notify_runner(&client, user_port)
        .await
        .context("Could not notify runner of ready status")?;

    // Initialize the event handler
    // TODO: persist payment info
    let inbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let outbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let event_handler = LdkEventHandler::new(
        network,
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
        peer_manager.clone(),
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
        network,
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
            network,
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

/// Initializes and validates a BitcoindClient given an LdkArgs
async fn bitcoind_client(
    args: &StartCommand,
) -> anyhow::Result<Arc<BitcoindClient>> {
    // NOTE could write a wrapper that does this printing automagically
    println!("Initializing bitcoind client");
    let new_res = BitcoindClient::new(
        args.bitcoind_rpc.host.clone(),
        args.bitcoind_rpc.port,
        args.bitcoind_rpc.username.clone(),
        args.bitcoind_rpc.password.clone(),
        Handle::current(),
    )
    .await;

    let client = match new_res {
        Ok(cli) => Arc::new(cli),
        Err(e) => bail!("Failed to connect to bitcoind client: {}", e),
    };

    // Check that the bitcoind we've connected to is running the network we
    // expect
    let bitcoind_chain = client.get_blockchain_info().await.chain;
    let chain_str = match args.network.into_inner() {
        bitcoin::Network::Bitcoin => "main",
        bitcoin::Network::Testnet => "test",
        bitcoin::Network::Regtest => "regtest",
        bitcoin::Network::Signet => "signet",
    };
    ensure!(
        bitcoind_chain == chain_str,
        "Chain argument ({}) didn't match bitcoind chain ({})",
        chain_str,
        bitcoind_chain,
    );

    println!("    bitcoind client done.");
    Ok(client)
}

/// Initializes a KeysManager (and grabs the node public key) based on sealed +
/// persisted data
async fn keys_manager(
    rng: &dyn SecureRandom,
    client: &reqwest::Client,
    user_id: UserId,
    measurement: &str,
) -> anyhow::Result<(PublicKey, Arc<KeysManager>)> {
    println!("Initializing keys manager");
    // Fetch our node pubkey, instance, and enclave data from the data store
    let (node_res, instance_res, enclave_res) = tokio::join!(
        api::get_node(client, user_id),
        api::get_instance(client, user_id, measurement.to_owned()),
        api::get_enclave(client, user_id, measurement.to_owned()),
    );
    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;
    let enclave_opt = enclave_res.context("Error while fetching enclave")?;

    let (pubkey, keys_manager) = match (node_opt, instance_opt, enclave_opt) {
        (None, None, None) => {
            // No node exists yet, create a new one
            println!("Generating new seed");
            let new_seed = rand::generate::<[u8; 32]>(rng).unwrap().expose();
            // TODO (sgx): Seal seed under this enclave's pubkey

            // Derive pubkey
            let keys_manager = keys_manager_from_seed(&new_seed);
            let pubkey = convert::derive_pubkey(&keys_manager)
                .context("Could not get derive our pubkey from seed")?;
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
            // TODO Derive from a subset of KEYREQUEST
            let enclave_id = format!("{}_{}", instance_id, "my_cpu_id");
            let enclave = Enclave {
                id: enclave_id,
                seed: new_seed.to_vec(),
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
            api::create_node_instance_enclave(client, node_instance_enclave)
                .await
                .context(
                    "Could not atomically create new node + instance + enclave",
                )?;

            (pubkey, keys_manager)
        }
        (None, Some(_instance), _) => {
            // This should never happen; the instance table is foreign keyed to
            // the node table
            bail!("Existing instances should always have existing nodes")
        }
        (None, None, Some(_enclave)) => {
            // This should never happen; the enclave table is foreign keyed to
            // the instance table
            bail!("Existing enclaves should always have existing instances")
        }
        (Some(_node), None, _) => {
            bail!("User has not provisioned this instance yet");
        }
        (Some(_node), Some(_instance), None) => {
            bail!("Provisioner should have sealed the seed for this enclave");
        }
        (Some(node), Some(_instance), Some(enclave)) => {
            println!("Found existing instance + enclave");

            // TODO(decrypt): Decrypt under enclave sealing key to get the seed
            let seed = enclave.seed;

            // Validate the seed
            ensure!(seed.len() == 32, "Incorrect seed length");
            let mut seed_buf = [0; 32];
            seed_buf.copy_from_slice(&seed);

            // Derive the node pubkey from the seed
            let keys_manager = keys_manager_from_seed(&seed_buf);
            let derived_pubkey = convert::derive_pubkey(&keys_manager)
                .context("Could not get derive our pubkey from seed")?;

            // Validate the pubkey returned from the DB against the derived one
            let given_pubkey = convert::pubkey_from_hex(&node.public_key)?;
            ensure!(
                given_pubkey == derived_pubkey,
                "Derived pubkey doesn't match the pubkey returned from the DB"
            );

            // Check the hex encodings as well
            let given_pubkey_hex = &node.public_key;
            let derived_pubkey_hex = &convert::pubkey_to_hex(&derived_pubkey);
            ensure!(
                given_pubkey_hex == derived_pubkey_hex,
                "Derived pubkey string doesn't match given pubkey string"
            );

            (derived_pubkey, keys_manager)
        }
    };

    println!("    keys manager done.");
    Ok((pubkey, Arc::new(keys_manager)))
}

/// Securely initializes a KeyManager from a given seed
fn keys_manager_from_seed(seed: &[u8; 32]) -> KeysManager {
    // FIXME(randomness): KeysManager::new() MUST be given a unique
    // `starting_time_secs` and `starting_time_nanos` for security. Since secure
    // timekeeping within an enclave is difficult, we should just take a
    // (securely) random u64, u32 instead. See KeysManager::new() for details.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Literally 1984");

    KeysManager::new(seed, now.as_secs(), now.subsec_nanos())
}

/// Initializes the ChannelMonitors
async fn channel_monitors(
    persister: &PostgresPersister,
    keys_manager: Arc<KeysManager>,
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
    persister: &PostgresPersister,
    bitcoind_client: &BitcoindClient,
    restarting_node: &mut bool,
    channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
    keys_manager: Arc<KeysManager>,
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
    args: &StartCommand,
    persister: &Arc<PostgresPersister>,
    logger: Arc<LdkTracingLogger>,
) -> anyhow::Result<(Arc<NetworkGraphType>, Arc<P2PGossipSyncType>)> {
    println!("Initializing gossip sync and network graph");
    let genesis = genesis_block(args.network.into_inner()).header.block_hash();

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

/// Initializes a PeerManager
fn peer_manager(
    rng: &dyn SecureRandom,
    keys_manager: &KeysManager,
    channel_manager: Arc<ChannelManagerType>,
    gossip_sync: Arc<P2PGossipSyncType>,
    logger: Arc<LdkTracingLogger>,
) -> anyhow::Result<Arc<PeerManagerType>> {
    let ephemeral_bytes = rand::generate::<[u8; 32]>(rng).unwrap().expose();
    let lightning_msg_handler = MessageHandler {
        chan_handler: channel_manager,
        route_handler: gossip_sync,
    };
    let peer_manager: PeerManagerType = PeerManagerType::new(
        lightning_msg_handler,
        keys_manager
            .get_node_secret(Recipient::Node)
            .map_err(|()| anyhow!("Could not get node secret"))?,
        &ephemeral_bytes,
        logger,
        Arc::new(IgnoringMessageHandler {}),
    );

    Ok(Arc::new(peer_manager))
}

/// Sets up a TcpListener to listen on 0.0.0.0:<listening_port>, handing off
/// resultant `TcpStream`s for the `PeerManager` to manage
fn spawn_p2p_listener(
    listening_port: Port,
    stop_listen: Arc<AtomicBool>,
    peer_manager: Arc<PeerManagerType>,
) {
    tokio::spawn(async move {
        let address = format!("0.0.0.0:{}", listening_port);
        let listener = tokio::net::TcpListener::bind(address)
			.await
			.expect("Failed to bind to listen port - is something else already listening on it?");
        loop {
            let (tcp_stream, _peer_addr) = listener.accept().await.unwrap();
            let tcp_stream = tcp_stream.into_std().unwrap();
            let peer_manager_clone = peer_manager.clone();
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
    peer_manager: Arc<PeerManagerType>,
    stop_listen_connect: Arc<AtomicBool>,
    persister: Arc<PostgresPersister>,
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
                                let _ = peer::do_connect_peer(
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
        let chain_poller = poll::ChainPoller::new(&mut derefed, network);
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
