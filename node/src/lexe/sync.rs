use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use bitcoin::BlockHash;
use common::cli::Network;
use common::shutdown::ShutdownChannel;
use lexe_ln::logger::LexeTracingLogger;
use lightning::chain::transaction::OutPoint;
use lightning::chain::{Listen, Watch};
use lightning_block_sync::poll::{ChainPoller, ValidatedBlockHeader};
use lightning_block_sync::{init as blocksyncinit, SpvClient};
use tokio::time::{self, Duration};
use tracing::{info, warn};

use crate::lexe::channel_manager::NodeChannelManager;
use crate::types::{
    BlockSourceType, BroadcasterType, ChainMonitorType,
    ChannelMonitorListenerType, ChannelMonitorType, FeeEstimatorType, LxTask,
};

/// How often the SpvClient client polls for an updated chain tip
const CHAIN_TIP_POLL_INTERVAL: Duration = Duration::from_secs(15);

/// Represents a fully synced channel manager and channel monitors. The process
/// of initialization completes the synchronization of the passed in chain
/// listeners to the latest chain tip. Finally, the object is consumed via
/// `feed_chain_monitor_and_spawn_spv()`, ending the synchronization process.
pub struct SyncedChainListeners {
    network: Network,
    block_source: Arc<BlockSourceType>,

    channel_manager: NodeChannelManager,
    cmcls: Vec<ChannelMonitorChainListener>,
    blockheader_cache: HashMap<BlockHash, ValidatedBlockHeader>,
    chain_tip: ValidatedBlockHeader,
}

impl SyncedChainListeners {
    #[allow(clippy::too_many_arguments)]
    pub async fn init_and_sync(
        network: Network,

        channel_manager: NodeChannelManager,
        channel_manager_blockhash: BlockHash,
        channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,

        block_source: Arc<BlockSourceType>,
        broadcaster: Arc<BroadcasterType>,
        fee_estimator: Arc<FeeEstimatorType>,
        logger: LexeTracingLogger,
        restarting_node: bool,
    ) -> anyhow::Result<Self> {
        if restarting_node {
            Self::from_existing(
                network,
                channel_manager,
                channel_manager_blockhash,
                channel_monitors,
                block_source,
                broadcaster,
                fee_estimator,
                logger,
            )
            .await
            .context("Could not sync existing node")
        } else {
            Self::from_new(network, channel_manager, block_source)
                .await
                .context("Could not sync new node")
        }
    }

    /// Syncs our existing channel manager and channel monitors to the latest
    /// chain tip. For these two types respectively, LDK impl's Listen for:
    ///
    /// - NodeChannelManager's inner type: `ChannelManagerType`
    /// - A 4-tuple (`ChannelMonitorListenerType`) which contains the channel
    ///   monitor (`ChannelMonitorType`) and handles to other actors.
    ///
    /// And we have to do some acrobatics to ensure `synchronize_listeners()`
    /// recognizes these impls.
    #[allow(clippy::too_many_arguments)]
    async fn from_existing(
        network: Network,

        channel_manager: NodeChannelManager,
        channel_manager_blockhash: BlockHash,
        channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,

        block_source: Arc<BlockSourceType>,
        broadcaster: Arc<BroadcasterType>,
        fee_estimator: Arc<FeeEstimatorType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Self> {
        println!("Syncing chain listeners");
        // This Vec holds (BlockHash, &impl Listen) because that's what the
        // `synchronize_listeners()` API requires. Just deal with it...
        let mut chain_listeners =
            Vec::with_capacity(channel_monitors.len() + 1);

        // Add the channel manager's blockhash and a ref to the inner
        // `ChannelManagerType` (which impls `Listen`) to the chain listeners
        chain_listeners.push((
            channel_manager_blockhash,
            channel_manager.deref() as &dyn Listen,
        ));

        // Construct channel monitor chain listeners from the channel monitors,
        // their blockhashes, and associated broadcasters / fee_estimators
        let mut cmcls = Vec::new();
        for (blockhash, channel_monitor) in channel_monitors {
            let cmcl = ChannelMonitorChainListener::new(
                channel_monitor,
                blockhash,
                broadcaster.clone(),
                fee_estimator.clone(),
                logger.clone(),
            );

            cmcls.push(cmcl);
        }

        // Add the channel monitor blockhashes and a reference to their
        // listeners to the chain listeners vec
        for cmcl in cmcls.iter() {
            chain_listeners
                .push((cmcl.blockhash, &cmcl.listener as &dyn Listen));
        }

        // We can now sync our chain listeners to the latest chain tip.
        let mut blockheader_cache = HashMap::new();
        let chain_tip = blocksyncinit::synchronize_listeners(
            &block_source.as_ref(),
            network.into_inner(),
            &mut blockheader_cache,
            chain_listeners,
        )
        .await
        // BlockSourceError doesn't impl std::error::Error but its innie does
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not synchronize chain listeners")?;

        println!("    chain listener sync done.");

        Ok(Self {
            network,
            block_source,
            channel_manager,
            cmcls,
            blockheader_cache,
            chain_tip,
        })
    }

    /// If this was a newly created node, meaning that we have 0 channel
    /// monitors and a `NodeChannelManager` initialized from scratch, our
    /// "SyncedChainListeners" consists of an empty
    /// `ChannelMonitorChainListener`s Vec along with the best validated block
    /// header from our block source.
    async fn from_new(
        network: Network,
        channel_manager: NodeChannelManager,
        block_source: Arc<BlockSourceType>,
    ) -> anyhow::Result<Self> {
        let chain_tip = blocksyncinit::validate_best_block_header(
            &mut block_source.deref(),
        )
        .await
        // BlockSourceError doesn't impl std::error::Error
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not validate best block header")?;

        let blockheader_cache = HashMap::new();

        // No persisted channel monitors => no channel monitor chain listeners.
        let cmcls = Vec::new();

        Ok(Self {
            network,
            block_source,
            channel_manager,
            cmcls,
            blockheader_cache,
            chain_tip,
        })
    }

    /// Consumes self, passing the synced channel monitors into the chain
    /// monitor so that it can watch the chain for closing transactions,
    /// fraudulent transactions, etc. Spawns a task for the SPV client to
    /// continue monitoring the chain.
    pub fn feed_chain_monitor_and_spawn_spv(
        mut self,
        chain_monitor: Arc<ChainMonitorType>,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<LxTask<()>> {
        for cmcl in self.cmcls {
            let (channel_monitor, funding_outpoint) =
                cmcl.into_monitor_and_outpoint();
            chain_monitor
                .watch_channel(funding_outpoint, channel_monitor)
                .map_err(|e| anyhow!("{:?}", e))
                .context("Could not pass channel monitor into chain monitor")?;
        }

        // Spawn the SPV client
        let spv_client_handle = LxTask::spawn(async move {
            // Need let binding o.w. the deref() ref doesn't live long enough
            let mut block_source_deref = self.block_source.deref();

            let chain_poller = ChainPoller::new(
                &mut block_source_deref,
                self.network.into_inner(),
            );
            let chain_listener = (chain_monitor, self.channel_manager);

            let mut spv_client = SpvClient::new(
                self.chain_tip,
                chain_poller,
                &mut self.blockheader_cache,
                &chain_listener,
            );

            let mut poll_timer = time::interval(CHAIN_TIP_POLL_INTERVAL);

            loop {
                tokio::select! {
                    _ = poll_timer.tick() => {
                        if let Err(e) = spv_client.poll_best_tip().await {
                            warn!("Error polling chain tip: {:#}", e.into_inner());
                        }
                    }
                    _ = shutdown.recv() =>
                        break info!("SPV client shutting down"),
                }
            }
        });

        Ok(spv_client_handle)
    }
}

/// Associates a ChannelMonitor `Listen` impl (`ChannelMonitorListenerType`)
/// with its latest synced block and funding outpoint. LDK calls this a
/// "ChainListener" (as opposed to just a Listener, i.e. the `Listen` impl)
pub struct ChannelMonitorChainListener {
    pub blockhash: BlockHash,
    pub listener: ChannelMonitorListenerType,
    pub funding_outpoint: OutPoint,
}

impl ChannelMonitorChainListener {
    pub fn new(
        channel_monitor: ChannelMonitorType,
        blockhash: BlockHash,
        broadcaster: Arc<BroadcasterType>,
        fee_estimator: Arc<FeeEstimatorType>,
        logger: LexeTracingLogger,
    ) -> Self {
        let (funding_outpoint, _script) = channel_monitor.get_funding_txo();

        // This tuple is ChannelMonitorListenerType, which LDK impls Listen for
        let listener = (channel_monitor, broadcaster, fee_estimator, logger);

        Self {
            blockhash,
            listener,
            funding_outpoint,
        }
    }

    /// Consumes self, returning the inner ChannelMonitor and funding outpoint.
    pub fn into_monitor_and_outpoint(self) -> (ChannelMonitorType, OutPoint) {
        let (channel_monitor, _broadcaster, _fee_est, _log) = self.listener;
        (channel_monitor, self.funding_outpoint)
    }
}
