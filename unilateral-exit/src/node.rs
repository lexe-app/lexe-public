//! The recovery node: a minimal close-and-sweep-only Lexe node.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, anyhow};
use lexe_api::cli::LspInfo;
use lexe_common::{ln::addr::LxSocketAddress, ppm::Ppm};
use lexe_crypto::rng::SysRng;
use lexe_ln::{
    alias::{BumpTransactionEventHandlerType, EsploraSyncClientType},
    background_processor,
    channel_monitor::ChannelMonitorPersister,
    esplora::LexeEsplora,
    event,
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    message_router::LexeMessageRouter,
    route::LexeRouter,
    sync::{self, BdkSyncRequest},
    tx_broadcaster::TxBroadcaster,
    usernode_config,
    wallet::{self, AddressScanParams, LexeCoinSelector, OnchainWallet},
};
use lexe_tokio::{
    DEFAULT_CHANNEL_SIZE, SMALLER_CHANNEL_SIZE,
    events_bus::EventsBus,
    notify,
    notify_once::NotifyOnce,
    task::{self, LxTask},
};
use lightning::{
    chain::chainmonitor::ChainMonitor,
    ln::peer_handler::IgnoringMessageHandler,
    routing::{
        gossip::NetworkGraph,
        scoring::{ProbabilisticScorer, ProbabilisticScoringDecayParameters},
    },
    sign::NodeSigner,
    util::wallet_utils,
};
use lightning_transaction_sync::EsploraSyncClient;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::{
    alias::OnionMessengerType,
    backups,
    event_handler::{EventCtx, RecoveryEventHandler},
    peer_manager::RecoveryPeerManager,
    persister::RecoveryPersister,
    recover::RecoveryCtx,
    types::{RecoveryArgs, RecoveryReport},
};

const FIRST_SYNC_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// The seed's keychains may have long histories with gaps, and the recovery
/// wallet starts with no local state, so scan much further than the usernode.
const RECOVERY_SCAN_PARAMS: AddressScanParams = AddressScanParams {
    full_scan_stop_gap: 50,
    lookahead: 50,
};
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

/// A minimal close-and-sweep-only Lexe node.
pub struct RecoveryNode {
    ctx: RecoveryCtx,
    tasks: Vec<LxTask<()>>,
}

/// Inputs to [`RecoveryNode::sync`].
pub struct SyncContext {
    bdk_resync_rx: mpsc::Receiver<BdkSyncRequest>,
    ldk_resync_rx: mpsc::Receiver<oneshot::Sender<()>>,
    ldk_sync_client: Arc<EsploraSyncClientType>,
    onchain_recv_tx: notify::Sender,
}

/// Inputs to [`RecoveryNode::run`].
pub struct RunContext {
    eph_tasks_rx: mpsc::Receiver<LxTask<()>>,
}

impl RecoveryNode {
    /// Imports the provided backups and assembles the recovery node,
    /// mirroring the usernode's init sequence minus all Lexe infrastructure.
    /// Once this returns, the recovery dir holds the imported backups, so
    /// the downloaded copies can be deleted.
    pub async fn init(
        args: RecoveryArgs,
        shutdown: NotifyOnce,
    ) -> anyhow::Result<(Self, SyncContext, RunContext)> {
        let mut rng = SysRng::new();
        let logger = LexeTracingLogger::new();
        let network = args.network;
        let mut tasks = Vec::new();
        let (eph_tasks_tx, eph_tasks_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);

        // Init persister over the local recovery dir.
        let (channel_monitor_persister_tx, channel_monitor_persister_rx) =
            mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let vfs_master_key = Arc::new(args.root_seed.derive_vfs_master_key());
        let persister = Arc::new(RecoveryPersister::new(
            channel_monitor_persister_tx.clone(),
            eph_tasks_tx.clone(),
            args.recovery_dir.clone(),
            shutdown.clone(),
            vfs_master_key,
        ));

        backups::import(
            &persister,
            args.channel_manager_path.as_deref(),
            &args.monitor_paths,
        )
        .await?;

        // Init esplora and fee estimates.
        let (esplora, fee_estimates, refresh_fees_task, _esplora_url) =
            LexeEsplora::init_any(
                "unilateral-exit",
                &mut rng,
                args.esplora_urls.clone(),
                network,
                shutdown.clone(),
            )
            .await
            .context("Failed to init esplora client")?;
        tasks.push(refresh_fees_task);
        let ldk_sync_client = Arc::new(EsploraSyncClient::from_client(
            esplora.client().clone(),
            logger.clone(),
        ));

        // Init BDK wallet.
        let (wallet_persister_tx, wallet_persister_rx) = notify::channel();
        let maybe_changeset = persister
            .read_wallet_changeset()
            .await
            .context("Failed to read wallet changeset")?;
        let master_xprv = args.root_seed.derive_bip32_master_xprv(network);
        let wallet = OnchainWallet::init(
            master_xprv,
            network,
            &esplora,
            fee_estimates.clone(),
            LexeCoinSelector::default(),
            maybe_changeset,
            RECOVERY_SCAN_PARAMS,
            wallet_persister_tx,
        )
        .await
        .context("Failed to init BDK wallet")?;
        tasks.push(wallet::spawn_wallet_persister_task(
            persister.clone(),
            wallet.clone(),
            wallet_persister_rx,
            shutdown.clone(),
        ));

        // Init tx broadcaster.
        let broadcast_hook = None;
        let test_event_sender = None;
        let (tx_broadcaster, broadcaster_task) = TxBroadcaster::start(
            esplora.clone(),
            wallet.clone(),
            broadcast_hook,
            test_event_sender,
            shutdown.clone(),
        );
        tasks.push(broadcaster_task);

        // Init keys manager and the anchor CPFP handler.
        let keys_manager =
            LexeKeysManager::new(&mut rng, &args.root_seed, wallet.clone())
                .map(Arc::new)
                .context("Failed to construct keys manager")?;
        // We delegate the fee-bumping itself to LDK's
        // `BumpTransactionEventHandler`, which builds, signs, and broadcasts
        // the anchor child txs, funding them with confirmed wallet UTXOs.
        let bump_transaction_handler = BumpTransactionEventHandlerType::new(
            tx_broadcaster.clone(),
            wallet_utils::Wallet::new(Arc::new(wallet.clone()), logger.clone()),
            keys_manager.clone(),
            logger.clone(),
        );

        // Init the chain monitor. Like the usernode, defer monitor updates
        // until after each channel manager persist.
        let chain_monitor = Arc::new(ChainMonitor::new(
            Some(ldk_sync_client.clone()),
            tx_broadcaster.clone(),
            logger.clone(),
            fee_estimates.clone(),
            persister.clone(),
            keys_manager.clone(),
            keys_manager.get_peer_storage_key(),
            /* deferred */ true,
        ));

        // Read and deserialize the imported channel monitors.
        let channel_monitor_bytes =
            lexe_ln::persister::read_channel_monitor_bytes(&persister)
                .await
                .context("Could not read channel monitor bytes")?;
        let mut channel_monitors =
            lexe_ln::persister::deserialize_channel_monitors(
                channel_monitor_bytes,
                &keys_manager,
            )
            .context("Could not deserialize channel monitors")?;

        // Reading the channel manager requires routers, but the recovery node
        // never routes payments. So the routers get an empty network graph and
        // a placeholder `LspInfo`, which uses our own node_pk if there's no
        // LSP.
        let network_graph =
            Arc::new(NetworkGraph::new(network.to_bitcoin(), logger.clone()));
        let scorer = Arc::new(Mutex::new(ProbabilisticScorer::new(
            ProbabilisticScoringDecayParameters::default(),
            network_graph.clone(),
            logger.clone(),
        )));
        let lsp_info = LspInfo {
            node_pk: args
                .close_mode
                .lsp()
                .map(|lsp| lsp.node_pk)
                .unwrap_or_else(|| keys_manager.node_pk()),
            private_p2p_addr: LxSocketAddress::TcpIpv4 {
                ip: std::net::Ipv4Addr::LOCALHOST,
                port: 9735,
            },
            lsp_usernode_base_fee_msat: 0,
            lsp_usernode_prop_fee: Ppm::ZERO,
            lsp_external_prop_fee: Ppm::ZERO,
            lsp_external_base_fee_msat: 0,
            cltv_expiry_delta: 0,
            htlc_minimum_msat: 0,
            htlc_maximum_msat: 0,
        };
        let intercept_scids = Vec::new();
        let router = Arc::new(LexeRouter::new_user_node(
            network_graph.clone(),
            logger.clone(),
            scorer.clone(),
            lsp_info.clone(),
            intercept_scids,
        ));
        let message_router = Arc::new(LexeMessageRouter::new_user_node(
            network_graph.clone(),
            lsp_info,
        ));

        // Read the channel manager with the exact usernode config.
        let config = usernode_config::user_config();
        let maybe_manager = lexe_ln::persister::read_channel_manager(
            &persister,
            config,
            &mut channel_monitors,
            keys_manager.clone(),
            fee_estimates.clone(),
            chain_monitor.clone(),
            tx_broadcaster.clone(),
            router.clone(),
            message_router.clone(),
            logger.clone(),
        )
        .await
        .context("Could not read channel manager")?;
        // A fresh manager would know none of the monitors' channels, so
        // nothing would generate the force-close updates that broadcast and
        // sweep them, and LDK would panic on their HTLC events. Thus the
        // persisted manager is required. Any node which has ever run has
        // one, so a missing manager means the backup is incomplete.
        let (best_block, channel_manager) = maybe_manager
            .context("No channel manager found; the backup is incomplete")?;
        let channel_manager = Arc::new(channel_manager);
        info!(
            blockhash = %best_block.block_hash,
            height = best_block.height,
            "Loaded channel manager"
        );

        for (_best_block, monitor) in channel_monitors {
            let channel_id = monitor.channel_id();
            chain_monitor
                .load_existing_monitor(channel_id, monitor)
                .map_err(|()| {
                    anyhow!("Failed to load channel monitor: {channel_id}")
                })?;
        }

        // Init onion messenger + peer manager.
        let offers_msg_handler = channel_manager.clone();
        let async_payments_msg_handler = IgnoringMessageHandler {};
        let dns_resolver = IgnoringMessageHandler {};
        let custom_onion_msg_handler = IgnoringMessageHandler {};
        let onion_messenger = Arc::new(OnionMessengerType::new(
            keys_manager.clone(),
            keys_manager.clone(),
            logger.clone(),
            channel_manager.clone(),
            message_router,
            offers_msg_handler,
            async_payments_msg_handler,
            dns_resolver,
            custom_onion_msg_handler,
        ));
        let (peer_manager, process_events_task) = RecoveryPeerManager::init(
            &mut rng,
            keys_manager.clone(),
            channel_manager.clone(),
            onion_messenger,
            chain_monitor.clone(),
            logger.clone(),
            shutdown.clone(),
        );
        tasks.push(process_events_task);

        // Init the event handler and event replayer.
        let channel_events_bus = EventsBus::new();
        let event_handler = RecoveryEventHandler {
            ctx: Arc::new(EventCtx {
                bump_transaction_handler,
                channel_events_bus: channel_events_bus.clone(),
                channel_manager: channel_manager.clone(),
                eph_tasks_tx: eph_tasks_tx.clone(),
                fee_estimates: fee_estimates.clone(),
                keys_manager: keys_manager.clone(),
                persister: persister.clone(),
                shutdown: shutdown.clone(),
                tx_broadcaster: tx_broadcaster.clone(),
                wallet: wallet.clone(),
            }),
        };
        tasks.push(event::spawn_event_replayer_task(event_handler.clone()));

        // Set up the channel monitor persistence task.
        let monitor_persister_shutdown = NotifyOnce::new();
        let backup_persister_shutdown = None;
        let max_active_persists = 4;
        tasks.push(
            ChannelMonitorPersister::new(
                persister.clone(),
                channel_manager.clone(),
                chain_monitor.clone(),
                channel_monitor_persister_rx,
                shutdown.clone(),
                monitor_persister_shutdown.clone(),
                backup_persister_shutdown,
                max_active_persists,
            )
            .spawn(),
        );

        // Init background processor. The recovery node never forwards, so no
        // forwarding delay.
        let forward_delay_range_ms = 0..1;
        let htlcs_forwarded_bus = EventsBus::new();
        let (bgp_control_tx, bgp_control_rx) =
            background_processor::control_channel();
        tasks.push(background_processor::start(
            channel_manager.clone(),
            peer_manager.clone(),
            persister.clone(),
            chain_monitor.clone(),
            channel_monitor_persister_tx,
            bgp_control_rx,
            event_handler,
            forward_delay_range_ms,
            htlcs_forwarded_bus,
            monitor_persister_shutdown,
            shutdown.clone(),
        ));

        // Set up the chain sync channels for `sync`.
        let (onchain_recv_tx, _onchain_recv_rx) = notify::channel();
        let (bdk_resync_tx, bdk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);
        let (ldk_resync_tx, ldk_resync_rx) =
            mpsc::channel(SMALLER_CHANNEL_SIZE);

        let ctx = RecoveryCtx {
            bdk_resync_tx,
            bgp_control_tx,
            chain_monitor,
            channel_events_bus,
            channel_manager,
            close_mode: args.close_mode,
            eph_tasks_tx,
            esplora,
            ldk_resync_tx,
            peer_manager,
            persister,
            progress_tx: args.progress_tx,
            shutdown,
            sync_interval: args.sync_interval,
            wallet,
        };
        let sync_ctx = SyncContext {
            bdk_resync_rx,
            ldk_resync_rx,
            ldk_sync_client,
            onchain_recv_tx,
        };
        let run_ctx = RunContext { eph_tasks_rx };
        Ok((Self { ctx, tasks }, sync_ctx, run_ctx))
    }

    /// Spawns the BDK + LDK chain sync tasks and waits for the first sync of
    /// each to complete.
    pub async fn sync(&mut self, sync_ctx: SyncContext) -> anyhow::Result<()> {
        let (first_bdk_sync_tx, first_bdk_sync_rx) = oneshot::channel();
        self.tasks.push(sync::spawn_bdk_sync_task(
            self.ctx.esplora.clone(),
            self.ctx.wallet.clone(),
            sync_ctx.onchain_recv_tx,
            first_bdk_sync_tx,
            sync_ctx.bdk_resync_rx,
            self.ctx.shutdown.clone(),
            FIRST_SYNC_TIMEOUT,
        ));

        let (first_ldk_sync_tx, first_ldk_sync_rx) = oneshot::channel();
        self.tasks.push(sync::spawn_ldk_sync_task(
            self.ctx.channel_manager.clone(),
            self.ctx.chain_monitor.clone(),
            sync_ctx.ldk_sync_client,
            first_ldk_sync_tx,
            sync_ctx.ldk_resync_rx,
            self.ctx.shutdown.clone(),
            FIRST_SYNC_TIMEOUT,
        ));

        let (try_first_bdk_sync, try_first_ldk_sync) =
            tokio::try_join!(first_bdk_sync_rx, first_ldk_sync_rx)
                .context("Sync task died before first sync")?;
        try_first_bdk_sync.context("Initial BDK sync failed")?;
        try_first_ldk_sync.context("Initial LDK sync failed")?;
        Ok(())
    }

    /// Runs the recovery to completion, then shuts the node down. The
    /// recovery closes all channels per [`CloseMode`] and waits for all
    /// funds to confirm on-chain.
    ///
    /// Returns early with an error if the `shutdown` given to
    /// [`init`](Self::init) is triggered. All progress is persisted to the
    /// recovery dir, so the run can simply be restarted.
    ///
    /// [`CloseMode`]: crate::types::CloseMode
    pub async fn run(
        self,
        run_ctx: RunContext,
    ) -> anyhow::Result<RecoveryReport> {
        let Self { mut ctx, mut tasks } = self;
        let shutdown = ctx.shutdown.clone();
        let (report_tx, report_rx) = oneshot::channel();

        // A dead node task must abort the recovery, so run the recovery as a
        // task alongside the node's tasks. However,
        // `try_join_tasks_and_shutdown` treats a task which finishes before
        // shutdown as an error, so the recovery task signals shutdown before
        // finishing.
        tasks.push(LxTask::spawn("recover", async move {
            let _ = report_tx.send(ctx.recover().await);
            ctx.shutdown.send();
            ctx.peer_manager.disconnect_all_peers();
        }));

        task::try_join_tasks_and_shutdown(
            tasks,
            run_ctx.eph_tasks_rx,
            shutdown,
            SHUTDOWN_TIMEOUT,
        )
        .await
        .context("Recovery node task failed")?;
        info!("Recovery node shut down");

        report_rx.await.context("Recovery task panicked")?
    }
}
