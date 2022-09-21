use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use bitcoin::BlockHash;
use common::api::provision::{Node, SealedSeedId};
use common::api::runner::{Port, UserPorts};
use common::api::UserPk;
use common::cli::RunArgs;
use common::client::tls::node_run_tls_config;
use common::enclave::{
    self, MachineId, Measurement, MinCpusvn, MIN_SGX_CPUSVN,
};
use common::rng::Crng;
use common::root_seed::RootSeed;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt};
use lexe_ln::alias::{
    BlockSourceType, BroadcasterType, ChannelMonitorType, FeeEstimatorType,
    NetworkGraphType, P2PGossipSyncType, WalletType,
};
use lexe_ln::bitcoind::LexeBitcoind;
use lexe_ln::init;
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::keysinterface::KeysInterface;
use lightning::onion_message::OnionMessenger;
use lightning::routing::gossip::P2PGossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, instrument, warn};

use crate::api::ApiClient;
use crate::event_handler::LdkEventHandler;
use crate::inactivity_timer::InactivityTimer;
use crate::lexe::background_processor::LexeBackgroundProcessor;
use crate::lexe::channel_manager::NodeChannelManager;
use crate::lexe::peer_manager::NodePeerManager;
use crate::lexe::persister::NodePersister;
use crate::lexe::sync::SyncedChainListeners;
use crate::types::{
    ApiClientType, ChainMonitorType, InvoicePayerType, PaymentInfoStorageType,
};
use crate::{api, command};

pub(crate) const DEFAULT_CHANNEL_SIZE: usize = 256;
// TODO(max): p2p stuff should probably go in its own module
const P2P_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);
/// The amount of time tasks have to finish after a graceful shutdown was
/// initiated before the program exits.
const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(15);

/// A user's node.
#[allow(dead_code)] // Many unread fields are used as type annotations
pub(crate) struct UserNode {
    // --- General --- //
    args: RunArgs,
    pub(crate) peer_port: Port,
    tasks: Vec<(&'static str, LxTask<()>)>,

    // --- Actors --- //
    pub(crate) channel_manager: NodeChannelManager,
    pub(crate) peer_manager: NodePeerManager,
    pub(crate) keys_manager: LexeKeysManager,
    pub(crate) persister: NodePersister,
    chain_monitor: Arc<ChainMonitorType>,
    pub(crate) network_graph: Arc<NetworkGraphType>,
    gossip_sync: Arc<P2PGossipSyncType>,
    invoice_payer: Arc<InvoicePayerType>,
    pub(crate) wallet: Arc<WalletType>,
    block_source: Arc<BlockSourceType>,
    broadcaster: Arc<BroadcasterType>,
    fee_estimator: Arc<FeeEstimatorType>,
    logger: LexeTracingLogger,
    inactivity_timer: InactivityTimer,

    // --- Sync --- //
    restarting_node: bool,
    channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,
    channel_manager_blockhash: BlockHash,

    // --- Run --- //
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
    shutdown: ShutdownChannel,
}

impl UserNode {
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

        // Init channels
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = ShutdownChannel::new();
        let (channel_monitor_updated_tx, channel_monitor_updated_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);

        // Initialize LexeBitcoind while fetching provisioned secrets
        let (bitcoind_res, fetch_res) = tokio::join!(
            LexeBitcoind::init(
                args.bitcoind_rpc.clone(),
                args.network,
                shutdown.clone(),
            ),
            fetch_provisioned_secrets(
                rng,
                api.as_ref(),
                user_pk,
                measurement,
                machine_id,
                min_cpusvn
            ),
        );
        let bitcoind =
            bitcoind_res.context("Failed to init bitcoind client")?;
        let (node, root_seed) =
            fetch_res.context("Failed to fetch provisioned secrets")?;
        let bitcoind = Arc::new(bitcoind);

        // Collect all handles to spawned tasks
        let mut tasks = Vec::with_capacity(10);

        // Spawn task to refresh feerates
        tasks.push(("refresh fees", bitcoind.spawn_refresh_fees_task()));

        // Build LexeKeysManager from node init data
        let keys_manager =
            LexeKeysManager::init(rng, &node.node_pk, &root_seed)
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

        // Initialize Persister
        let persister = NodePersister::new(
            api.clone(),
            node_pk,
            measurement,
            shutdown.clone(),
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
        let channel_monitor_updated_task =
            init::spawn_channel_monitor_updated_task(
                chain_monitor.clone(),
                channel_monitor_updated_rx,
                shutdown.clone(),
            );
        tasks.push(("channel monitor updated", channel_monitor_updated_task));

        // Read channel monitors while reading network graph
        let (channel_monitors_res, network_graph_res) = tokio::join!(
            persister.read_channel_monitors(keys_manager.clone()),
            persister.read_network_graph(args.network, logger.clone())
        );
        let mut channel_monitors =
            channel_monitors_res.context("Could not read channel monitors")?;
        let network_graph =
            network_graph_res.context("Could not read network graph")?;
        let network_graph = Arc::new(network_graph);

        // Init gossip sync
        let gossip_sync =
            P2PGossipSync::new(network_graph.clone(), None, logger.clone());
        let gossip_sync = Arc::new(gossip_sync);

        // Initialize the ChannelManager and ProbabilisticScorer
        let mut restarting_node = true;
        let (channel_manager_res, scorer_res) = tokio::join!(
            NodeChannelManager::init(
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

        // Init onion messenger
        let onion_messenger =
            Arc::new(OnionMessenger::new(keys_manager.clone(), logger.clone()));

        // Initialize PeerManager
        let peer_manager = NodePeerManager::init(
            rng,
            &keys_manager,
            channel_manager.clone(),
            gossip_sync.clone(),
            onion_messenger,
            logger.clone(),
        );

        // Set up listening for inbound P2P connections
        let (p2p_listener_task, peer_port) = spawn_p2p_listener(
            peer_manager.clone(),
            args.peer_port,
            shutdown.clone(),
        )
        .await;
        tasks.push(("p2p listener", p2p_listener_task));

        // Spawn a task to regularly reconnect to channel peers
        let p2p_reconnector_task = spawn_p2p_reconnector(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            shutdown.clone(),
        );
        tasks.push(("p2p reconnectooor", p2p_reconnector_task));

        // Build owner service TLS config for authenticating owner
        let node_dns = args.node_dns_name.clone();
        let owner_tls = node_run_tls_config(rng, &root_seed, vec![node_dns])
            .context("Failed to build owner service TLS config")?;

        // Start warp service for owner
        let owner_routes = command::server::owner_routes(
            channel_manager.clone(),
            peer_manager.clone(),
            network_graph.clone(),
            activity_tx,
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
        let owner_service_task = LxTask::spawn(async move {
            owner_service_fut.await;
        });
        tasks.push(("owner service", owner_service_task));

        // TODO(phlip9): authenticate host<->node
        // Start warp service for host
        let host_routes =
            command::server::host_routes(args.user_pk, shutdown.clone());
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
        let host_service_task = LxTask::spawn(async move {
            host_service_fut.await;
        });
        tasks.push(("host service", host_service_task));

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
        let bg_processor_task = LexeBackgroundProcessor::start(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            invoice_payer.clone(),
            gossip_sync.clone(),
            scorer.clone(),
            shutdown.clone(),
        );
        tasks.push(("background processor", bg_processor_task));

        // Construct (but don't start) the inactivity timer
        let inactivity_timer = InactivityTimer::new(
            args.shutdown_after_sync_if_no_activity,
            args.inactivity_timer_sec,
            activity_rx,
            shutdown.clone(),
        );

        // Build and return the UserNode
        let node = UserNode {
            // General
            args,
            peer_port,
            tasks,

            // Actors
            channel_manager,
            peer_manager,
            keys_manager,
            persister,
            chain_monitor,
            network_graph,
            gossip_sync,
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
            shutdown,
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

        // Populate the chain monitor and spawn the SPV client
        let spv_client_task = synced_chain_listeners
            .feed_chain_monitor_and_spawn_spv(
                self.chain_monitor.clone(),
                self.shutdown.clone(),
            )
            .context("Error wrapping up sync")?;
        self.tasks.push(("spv client", spv_client_task));

        // Sync is complete; start the inactivity timer.
        debug!("Starting inactivity timer");
        let inactivity_timer_task = LxTask::spawn(async move {
            self.inactivity_timer.start().await;
        });
        self.tasks.push(("inactivity timer", inactivity_timer_task));

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
        self.shutdown.recv().await;

        // --- Shutdown --- //
        info!("Main thread shutting down.");

        info!("Disconnecting all peers.");
        // This ensures we don't continue updating our channel data after we've
        // stopped the background processor.
        self.peer_manager.disconnect_all_peers();

        info!("Waiting on all tasks to finish");
        let mut tasks = self
            .tasks
            .into_iter()
            .map(|(name, task)| async move {
                let join_res = task.await;
                (name, join_res)
            })
            .collect::<FuturesUnordered<_>>();
        let timeout = tokio::time::sleep(SHUTDOWN_TIME_LIMIT);
        tokio::pin!(timeout);
        while !tasks.is_empty() {
            tokio::select! {
                Some((name, join_res)) = tasks.next() => {
                    match join_res {
                        Ok(()) => info!("'{name}' task finished"),
                        Err(e) => error!("'{name}' task panicked: {e:#}"),
                    }
                }
                () = &mut timeout => {
                    warn!("{} tasks failed to finish", tasks.len());
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Constructs a Arc<dyn ApiClient> based on whether we are running in SGX,
/// and whether `args.mock` is set to true
fn init_api(args: &RunArgs) -> ApiClientType {
    // Production can only use the real api client
    #[cfg(all(target_env = "sgx", not(test)))]
    {
        Arc::new(api::NodeApiClient::new(
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
            Arc::new(api::NodeApiClient::new(
                args.backend_url.clone(),
                args.runner_url.clone(),
            ))
        }
    }
}

/// Fetches previously provisioned secrets from the API.
async fn fetch_provisioned_secrets<R: Crng>(
    rng: &mut R,
    api: &dyn ApiClient,
    user_pk: UserPk,
    measurement: Measurement,
    machine_id: MachineId,
    min_cpusvn: MinCpusvn,
) -> anyhow::Result<(Node, RootSeed)> {
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

            let sealed_seed = api
                .get_sealed_seed(sealed_seed_id)
                .await
                .context("Error while fetching sealed seed")?
                .context("Sealed seed wasn't persisted with node & instance")?;

            let root_seed = sealed_seed
                .unseal_and_validate(rng)
                .context("Could not validate or unseal sealed seed")?;

            Ok((node, root_seed))
        }
        (None, None) => bail!("Enclave version has not been provisioned yet"),
        _ => bail!("Node and instance should have been persisted together"),
    }
}

/// Sets up a `TcpListener` to listen on 0.0.0.0:<peer_port>, handing off
/// resultant `TcpStream`s for the `PeerManager` to manage
async fn spawn_p2p_listener(
    peer_manager: NodePeerManager,
    peer_port_opt: Option<Port>,
    mut shutdown: ShutdownChannel,
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
        let mut child_tasks = FuturesUnordered::new();

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
                    let child_task = LxTask::spawn(async move {
                        // `setup_inbound()` returns a future that completes
                        // when the connection is closed. The main thread calls
                        // peer_manager.disconnect_all_peers() once it receives
                        // a shutdown signal so there is no need to pass in a
                        // `shutdown`s here.
                        let connection_closed = lightning_net_tokio::setup_inbound(
                            peer_manager_clone,
                            tcp_stream,
                        );
                        connection_closed.await;
                    });

                    child_tasks.push(child_task);
                }
                // To prevent a memory leak of LxTasks, we select! on the
                // futures unordered so that we can clear out LxTasks for peers
                // that disconnect before the node shuts down.
                Some(join_res) = child_tasks.next() => {
                    if let Err(e) = join_res {
                        error!("P2P connection task panicked: {e:#}");
                    }
                }
                () = shutdown.recv() =>
                    break info!("LN P2P listen task shutting down"),
            }
        }

        // Wait on all child tasks to finish (i.e. all connections close).
        while let Some(join_res) = child_tasks.next().await {
            if let Err(e) = join_res {
                error!("P2P connection task panicked: {:#}", e);
            }
        }

        info!("LN P2P listen task complete");
    });

    (handle, peer_port)
}

/// Spawns a task that regularly reconnects to the channel peers stored in DB.
fn spawn_p2p_reconnector(
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    persister: NodePersister,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn(async move {
        let mut interval = time::interval(P2P_RECONNECT_INTERVAL);

        loop {
            interval.tick().await;
            tokio::select! {
                // Prevents race condition where we initiate a reconnect *after*
                // a shutdown signal was received, causing this task to hang
                biased;
                () = shutdown.recv() => break,
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
                    warn!("Couldn't reconnect to channel peer: {:#}", e)
                }
            }
        }

        info!("p2p reconnector task shut down");
    })
}
