use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::secp256k1::PublicKey;
use bitcoin::BlockHash;

use lightning::chain::chainmonitor;
use lightning::chain::keysinterface::{KeysInterface, KeysManager, Recipient};
use lightning::chain::transaction::OutPoint;
use lightning::chain::{self, BestBlock, Watch};
use lightning::ln::channelmanager;
use lightning::ln::channelmanager::ChainParameters;
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::routing::gossip::P2PGossipSync;
use lightning::util::config::UserConfig;
use lightning::util::events::Event;
use lightning_background_processor::BackgroundProcessor;
use lightning_block_sync::init as blocksyncinit;
use lightning_block_sync::poll::{self, ValidatedBlockHeader};
use lightning_block_sync::SpvClient;
use lightning_block_sync::UnboundedCache;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;

use anyhow::{anyhow, bail, ensure, Context};
use rand::Rng;
use tokio::runtime::Handle;
use warp::Filter as WarpFilter;

use crate::api::{
    self, Enclave, Instance, Node, NodeInstanceEnclave, UserPort,
};
use crate::bitcoind_client::BitcoindClient;
use crate::cli;
use crate::convert;
use crate::event_handler;
use crate::logger::StdOutLogger;
use crate::persister::PostgresPersister;
use crate::structs::{LdkArgs, LexeArgs};
use crate::types::{
    ChainMonitorType, ChannelManagerType, ChannelMonitorListenerType,
    ChannelMonitorType, GossipSyncType, InvoicePayerType, NetworkGraphType,
    P2PGossipSyncType, PaymentInfoStorageType, PeerManagerType, UserId,
};

pub async fn start_ldk() -> anyhow::Result<()> {
    // Parse command line args
    let lexe_args: LexeArgs = argh::from_env();
    let args: LdkArgs = lexe_args
        .try_into()
        .context("Could not parse command line args")?;

    // Initialize the Logger
    let logger = Arc::new(StdOutLogger {});

    // Get user_id, measurement, and HTTP client, used throughout init
    let user_id = args.user_id;
    // TODO(sgx) Insert this enclave's measurement
    let measurement = String::from("default");
    let client = reqwest::Client::new();

    // Initialize BitcoindClient and KeysManager
    // tokio::join! doesn't "fail fast" but produces better error chains
    let (bitcoind_client_res, keys_manager_res) = tokio::join!(
        bitcoind_client(&args),
        keys_manager(&client, user_id, &measurement),
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

    // Initialize the `P2PGossipSync` and `ChannelMonitor`s
    let (channel_monitors_res, gossip_sync_res) = tokio::join!(
        channel_monitors(&persister, &keys_manager),
        gossip_sync(&args, &persister, logger.clone())
    );
    let mut channel_monitors =
        channel_monitors_res.context("Could not read channel monitors")?;
    let (network_graph, gossip_sync) =
        gossip_sync_res.context("Could not initialize gossip sync")?;

    // Initialize the ChannelManager
    let mut restarting_node = true;
    let (channel_manager_blockhash, channel_manager) = channel_manager(
        &args,
        &persister,
        &mut channel_monitors,
        &keys_manager,
        &fee_estimator,
        &chain_monitor,
        &broadcaster,
        &logger,
        &bitcoind_client,
        &mut restarting_node,
    )
    .await
    .context("Could not init ChannelManager")?;

    // Sync channel_monitors and ChannelManager to chain tip
    let mut blockheader_cache = UnboundedCache::new();
    let (chain_listener_channel_monitors, chain_tip) = if restarting_node {
        sync_chain_listeners(
            &args,
            &channel_manager,
            &bitcoind_client,
            &broadcaster,
            &fee_estimator,
            &logger,
            channel_manager_blockhash,
            channel_monitors,
            &mut blockheader_cache,
        )
        .await
        .context("Could not sync channel listeners")?
    } else {
        let clcm = Vec::new();
        let chain_tip = blocksyncinit::validate_best_block_header(
            &mut bitcoind_client.deref(),
        )
        .await
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not validate best block header")?;
        (clcm, chain_tip)
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

    // Step 12: Initialize the PeerManager
    let peer_manager = peer_manager(
        keys_manager.as_ref(),
        channel_manager.clone(),
        gossip_sync.clone(),
        logger.clone(),
    )
    .context("Could not initialize peer manager")?;

    // ## Running LDK
    // Step 13: Initialize networking

    let peer_manager_connection_handler = peer_manager.clone();
    let listening_port = args.peer_port;
    let stop_listen_connect = Arc::new(AtomicBool::new(false));
    let stop_listen = Arc::clone(&stop_listen_connect);
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", listening_port))
			.await
			.expect("Failed to bind to listen port - is something else already listening on it?");
        loop {
            let peer_mgr = peer_manager_connection_handler.clone();
            let (tcp_stream, _peer_addr) = listener.accept().await.unwrap();
            let tcp_stream = tcp_stream.into_std().unwrap();
            if stop_listen.load(Ordering::Acquire) {
                return;
            }
            tokio::spawn(async move {
                lightning_net_tokio::setup_inbound(
                    peer_mgr.clone(),
                    tcp_stream,
                )
                .await;
            });
        }
    });

    // Step 14: Connect and Disconnect Blocks
    let channel_manager_listener = channel_manager.clone();
    let chain_monitor_listener = chain_monitor.clone();
    let bitcoind_block_source = bitcoind_client.clone();
    let network = args.network;
    tokio::spawn(async move {
        let mut derefed = bitcoind_block_source.deref();
        let chain_poller = poll::ChainPoller::new(&mut derefed, network);
        let chain_listener = (chain_monitor_listener, channel_manager_listener);
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

    // Step 15: Handle LDK Events
    let channel_manager_event_listener = channel_manager.clone();
    let keys_manager_listener = keys_manager.clone();
    // TODO: persist payment info to disk
    let inbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let outbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let inbound_pmts_for_events = inbound_payments.clone();
    let outbound_pmts_for_events = outbound_payments.clone();
    let network = args.network;
    let bitcoind_rpc = bitcoind_client.clone();
    let network_graph_events = network_graph.clone();
    let handle = tokio::runtime::Handle::current();
    let event_handler = move |event: &Event| {
        handle.block_on(event_handler::handle_ldk_events(
            &channel_manager_event_listener,
            &bitcoind_rpc,
            &network_graph_events,
            &keys_manager_listener,
            &inbound_pmts_for_events,
            &outbound_pmts_for_events,
            network,
            event,
        ));
    };

    // Step 16: Initialize routing ProbabilisticScorer
    let scorer = persister
        .read_probabilistic_scorer(
            Arc::clone(&network_graph),
            Arc::clone(&logger),
        )
        .await
        .context("Could not read probabilistic scorer")?;
    let scorer = Arc::new(Mutex::new(scorer));

    // Step 17: Create InvoicePayer
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

    // Step 18: Background Processing
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

    // Regularly reconnect to channel peers.
    let connect_channel_manager = Arc::clone(&channel_manager);
    let connect_peer_manager = Arc::clone(&peer_manager);
    let stop_connect = Arc::clone(&stop_listen_connect);
    let connect_persister = Arc::clone(&persister);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            match connect_persister.read_channel_peers().await {
                Ok(cp_vec) => {
                    let peers = connect_peer_manager.get_peer_node_ids();
                    for node_id in connect_channel_manager
                        .list_channels()
                        .iter()
                        .map(|chan| chan.counterparty.node_id)
                        .filter(|id| !peers.contains(id))
                    {
                        if stop_connect.load(Ordering::Acquire) {
                            return;
                        }
                        for (pubkey, peer_addr) in cp_vec.iter() {
                            if *pubkey == node_id {
                                let _ = cli::do_connect_peer(
                                    *pubkey,
                                    *peer_addr,
                                    Arc::clone(&connect_peer_manager),
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

    // Regularly broadcast our node_announcement. This is only required (or
    // possible) if we have some public channels, and is only useful if we have
    // public listen address(es) to announce. In a production environment, this
    // should occur only after the announcement of new channels to avoid churn
    // in the global network graph.
    let chan_manager = Arc::clone(&channel_manager);
    let network = args.network;
    if !args.ldk_announced_listen_addr.is_empty() {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                chan_manager.broadcast_node_announcement(
                    [0; 3],
                    args.ldk_announced_node_name,
                    args.ldk_announced_listen_addr.clone(),
                );
            }
        });
    }

    // Start warp at the given port
    println!("Serving warp at port {}", args.warp_port);
    tokio::spawn(async move {
        warp::serve(warp::path::end().map(|| "This is a Lexe user node"))
            .run(([127, 0, 0, 1], args.warp_port))
            .await;
    });

    // Let the runner know that we're ready
    let user_port = UserPort {
        user_id,
        port: args.warp_port,
    };
    println!("\n\nNotifying runner\n\n"); // \n o.w. its gets buried in stdout
    api::notify_runner(&client, user_port)
        .await
        .context("Could not notify runner of ready status")?;

    // Start the CLI.
    cli::poll_for_user_input(
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
    args: &LdkArgs,
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
    let chain_str = match args.network {
        bitcoin::Network::Bitcoin => "main",
        bitcoin::Network::Testnet => "test",
        bitcoin::Network::Regtest => "regtest",
        bitcoin::Network::Signet => "signet",
    };
    ensure!(
        bitcoind_chain == chain_str,
        anyhow!(
            "Chain argument ({}) didn't match bitcoind chain ({})",
            args.network,
            bitcoind_chain
        )
    );

    println!("    Initialized bitcoind client.");
    Ok(client)
}

/// Initializes a KeysManager (and grabs the node public key) based on sealed +
/// persisted data
async fn keys_manager(
    client: &reqwest::Client,
    user_id: UserId,
    measurement: &str,
) -> anyhow::Result<(PublicKey, Arc<KeysManager>)> {
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
            let mut new_seed = [0; 32];
            rand::thread_rng().fill_bytes(&mut new_seed);
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
    persister: &Arc<PostgresPersister>,
    keys_manager: &Arc<KeysManager>,
) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
    persister
        .read_channel_monitors(keys_manager.clone())
        .await
        .context("Could not read channel monitors")
}

/// Initializes the ChannelManager
#[allow(clippy::too_many_arguments)]
async fn channel_manager(
    args: &LdkArgs,
    persister: &Arc<PostgresPersister>,
    channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
    keys_manager: &Arc<KeysManager>,
    fee_estimator: &Arc<BitcoindClient>,
    chain_monitor: &Arc<ChainMonitorType>,
    broadcaster: &Arc<BitcoindClient>,
    logger: &Arc<StdOutLogger>,
    bitcoind_client: &Arc<BitcoindClient>,
    restarting_node: &mut bool,
) -> anyhow::Result<(BlockHash, Arc<ChannelManagerType>)> {
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
                network: args.network,
                best_block: BestBlock::new(
                    getinfo_resp.latest_blockhash,
                    getinfo_resp.latest_height as u32,
                ),
            };
            let fresh_channel_manager = channelmanager::ChannelManager::new(
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                logger.clone(),
                keys_manager.clone(),
                user_config,
                chain_params,
            );
            (getinfo_resp.latest_blockhash, fresh_channel_manager)
        }
    };
    let channel_manager = Arc::new(channel_manager);

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
    args: &LdkArgs,
    channel_manager: &ChannelManagerType,
    bitcoind_client: &Arc<BitcoindClient>,
    broadcaster: &Arc<BitcoindClient>,
    fee_estimator: &Arc<BitcoindClient>,
    logger: &Arc<StdOutLogger>,
    channel_manager_blockhash: BlockHash,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    blockheader_cache: &mut HashMap<BlockHash, ValidatedBlockHeader>,
) -> anyhow::Result<(Vec<ChannelMonitorChainListener>, ValidatedBlockHeader)> {
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
        &&**bitcoind_client,
        args.network,
        blockheader_cache,
        chain_listeners,
    )
    .await
    .map_err(|e| anyhow!(e.into_inner()))
    .context("Could not synchronize chain listeners")?;

    Ok((chain_listener_channel_monitors, chain_tip))
}

/// Initializes a GossipSync and NetworkGraph
async fn gossip_sync(
    args: &LdkArgs,
    persister: &Arc<PostgresPersister>,
    logger: Arc<StdOutLogger>,
) -> anyhow::Result<(Arc<NetworkGraphType>, Arc<P2PGossipSyncType>)> {
    let genesis = genesis_block(args.network).header.block_hash();

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

    Ok((network_graph, gossip_sync))
}

/// Initializes a PeerManager
fn peer_manager(
    keys_manager: &KeysManager,
    channel_manager: Arc<ChannelManagerType>,
    gossip_sync: Arc<P2PGossipSyncType>,
    logger: Arc<StdOutLogger>,
) -> anyhow::Result<Arc<PeerManagerType>> {
    let mut ephemeral_bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut ephemeral_bytes);
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
