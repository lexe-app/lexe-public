use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use bitcoin::blockdata::block::BlockHeader;
use bitcoin::BlockHash;
use common::cli::Network;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::transaction::{OutPoint, TransactionData};
use lightning::chain::{Listen, Watch};
use lightning_block_sync::poll::{ChainPoller, ValidatedBlockHeader};
use lightning_block_sync::{init as block_sync_init, SpvClient};
use tokio::time::{self, Duration};
use tracing::{info, warn};

use crate::alias::{
    BlockSourceType, BroadcasterType, ChannelMonitorListenerType,
    ChannelMonitorType, FeeEstimatorType, LexeChainMonitorType,
    LexeChannelManagerType,
};
use crate::logger::LexeTracingLogger;
use crate::traits::LexePersister;

/// How often the [`SpvClient`] client polls for an updated chain tip.
const CHAIN_TIP_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Represents a fully synced channel manager and channel monitors. The process
/// of initialization completes the synchronization of the passed in chain
/// listeners to the latest chain tip. Finally, the object is consumed via
/// `feed_chain_monitor_and_spawn_spv()`, ending the synchronization process.
///
/// The code in this module is confusing so here are some clarifying notes:
///
/// - LDK refers to an implementor of the [`Listen`] trait as a "Listener".
/// - LDK implements [`Listen`] for [`ChannelManager`] and
///   [`ChannelMonitorListenerType`], the latter of which is a 4-tuple composed
///   of the channel monitor ([`ChannelMonitorType`]) and handles to other
///   actors.
/// - The `LxListener` enum encapsulates both of these, implementing [`Listen`]
///   by delegating to its inner listener.
/// - Each "Listener" is associated with a [`BlockHash`] representing the latest
///   chain tip it has been synced to.
/// - [`lightning_block_sync::init::synchronize_listeners`] takes a `(BlockHash,
///   &impl Listen)` as input.
/// - `ldk-sample` forms the `(BlockHash, &impl Listen)`  by casting the
///   [`ChannelManager`] or [`ChannelMonitor`] into a `&dyn Listen`, but we had
///   to change from this to the concrete `LxListener` enum because it was
///   preventing Lexe nodes from being [`Send`], which is required for running
///   the nodes inside Tokio tasks for smoketests and other integration tests.
///
/// [`ChannelManager`]: lightning::ln::channelmanager::ChannelManager
/// [`ChannelMonitor`]: lightning::chain::channelmonitor::ChannelMonitor
#[must_use]
pub struct SyncedChainListeners<PERSISTER: LexePersister> {
    network: Network,
    block_source: Arc<BlockSourceType>,

    channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
    chain_listeners: Vec<LxChainListener<PERSISTER>>,
    blockheader_cache: HashMap<BlockHash, ValidatedBlockHeader>,
    chain_tip: ValidatedBlockHeader,
}

impl<PERSISTER: LexePersister> SyncedChainListeners<PERSISTER> {
    #[allow(clippy::too_many_arguments)]
    pub async fn init_and_sync(
        network: Network,

        channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
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
    /// chain tip. This function is mostly acrobatics to transform everything
    /// into the parameters required by [`synchronize_listeners`].
    ///
    /// [`synchronize_listeners`]: block_sync_init::synchronize_listeners
    #[allow(clippy::too_many_arguments)]
    async fn from_existing(
        network: Network,

        channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
        channel_manager_blockhash: BlockHash,
        channel_monitors: Vec<(BlockHash, ChannelMonitorType)>,

        block_source: Arc<BlockSourceType>,
        broadcaster: Arc<BroadcasterType>,
        fee_estimator: Arc<FeeEstimatorType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Self> {
        info!("Syncing chain listeners");

        // This Vec holds owned `LxChainListener`s.
        let mut chain_listeners =
            Vec::with_capacity(channel_monitors.len() + 1);

        // Add the channel manager to the chain listeners vec
        let channel_manager_lx_chain_listener = LxChainListener {
            blockhash: channel_manager_blockhash,
            listener: LxListener::ChannelManager(channel_manager.clone()),
        };
        chain_listeners.push(channel_manager_lx_chain_listener);

        // Add the chain monitors to the chain listeners vec
        for (blockhash, channel_monitor) in channel_monitors {
            let cmcl = ChannelMonitorChainListener::new(
                channel_monitor,
                broadcaster.clone(),
                fee_estimator.clone(),
                logger.clone(),
            );
            let listener = LxListener::ChannelMonitor(cmcl);
            let channel_monitor_lx_chain_listener = LxChainListener {
                blockhash,
                listener,
            };
            chain_listeners.push(channel_monitor_lx_chain_listener);
        }

        // Inherited from ldk-sample, BlockSource is impl'd on &LexeBitcoind.
        // LDK blocksync then requires a ref to the impl, hence &&LexeBitcoind.
        let block_source_ref_ref = &block_source.as_ref();

        // Now, build a Vec<(BlockHash, &impl Listen)> which LDK requires.
        let chain_listener_refs = chain_listeners
            .iter()
            .map(|chain_listener| {
                (chain_listener.blockhash, &chain_listener.listener)
            })
            .collect::<Vec<(BlockHash, &LxListener<PERSISTER>)>>();

        // Block header cache which is required for the SPV client init later.
        let mut blockheader_cache = HashMap::new();

        // We can now sync our chain listeners to the latest chain tip.
        let chain_tip = block_sync_init::synchronize_listeners(
            block_source_ref_ref,
            network.into_inner(),
            &mut blockheader_cache,
            chain_listener_refs,
        )
        .await
        // BlockSourceError doesn't impl std::error::Error but its innie does
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not synchronize chain listeners")?;

        info!("Syncing chain listeners complete.");

        Ok(Self {
            network,
            block_source,
            channel_manager,
            chain_listeners,
            blockheader_cache,
            chain_tip,
        })
    }

    /// If this was a newly created node, meaning that we have 0 channel
    /// monitors and a channel manager initialized from scratch, our
    /// "SyncedChainListeners" consists of an empty
    /// `ChannelMonitorChainListener`s Vec along with the best validated block
    /// header from our block source.
    async fn from_new(
        network: Network,
        channel_manager: Arc<LexeChannelManagerType<PERSISTER>>,
        block_source: Arc<BlockSourceType>,
    ) -> anyhow::Result<Self> {
        let chain_tip = block_sync_init::validate_best_block_header(
            &mut block_source.deref(),
        )
        .await
        // BlockSourceError doesn't impl std::error::Error
        .map_err(|e| anyhow!(e.into_inner()))
        .context("Could not validate best block header")?;

        let blockheader_cache = HashMap::new();

        // No persisted channel monitors => no channel monitor chain listeners.
        let chain_listeners = Vec::new();

        Ok(Self {
            network,
            block_source,
            channel_manager,
            chain_listeners,
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
        chain_monitor: Arc<LexeChainMonitorType<PERSISTER>>,
        mut shutdown: ShutdownChannel,
    ) -> anyhow::Result<LxTask<()>> {
        for chain_listener in self.chain_listeners {
            if let LxListener::ChannelMonitor(cmcl) = chain_listener.listener {
                let (channel_monitor, funding_outpoint) =
                    cmcl.into_monitor_and_outpoint();
                chain_monitor
                    .watch_channel(funding_outpoint, channel_monitor)
                    .map_err(|e| anyhow!("{e:?}"))
                    .context(
                        "Couldn't pass channel monitor to chain monitor",
                    )?;
            }
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

/// Associates a [`LxListener`] with its latest synced [`BlockHash`].
struct LxChainListener<PERSISTER: LexePersister> {
    blockhash: BlockHash,
    listener: LxListener<PERSISTER>,
}

/// Concretely enumerates the different kinds of `impl Listen`. This enum is
/// required because passing in `&dyn Listen` into
/// [`lightning_block_sync::init::synchronize_listeners`] (as ldk-sample does)
/// causes this sync implementation to not be [`Send`], which is required for
/// moving the node into a task spawned during smoketests.
enum LxListener<PERSISTER: LexePersister> {
    ChannelMonitor(ChannelMonitorChainListener),
    ChannelManager(Arc<LexeChannelManagerType<PERSISTER>>),
}

/// This [`Listen`] impl simply delegates to the inner type.
impl<PERSISTER: LexePersister> Listen for LxListener<PERSISTER> {
    fn filtered_block_connected(
        &self,
        header: &BlockHeader,
        txdata: &TransactionData<'_>,
        height: u32,
    ) {
        match self {
            Self::ChannelMonitor(cmcl) => cmcl
                .listener
                .filtered_block_connected(header, txdata, height),
            Self::ChannelManager(cm) => {
                cm.deref().filtered_block_connected(header, txdata, height)
            }
        }
    }

    fn block_disconnected(&self, header: &BlockHeader, height: u32) {
        match self {
            Self::ChannelMonitor(cmcl) => {
                cmcl.listener.block_disconnected(header, height)
            }
            Self::ChannelManager(cm) => {
                cm.deref().block_disconnected(header, height)
            }
        }
    }
}

/// Associates a ChannelMonitor [`Listen`] impl (`ChannelMonitorListenerType`)
/// with its funding outpoint. This struct is defined mostly to prevent the
/// chain sync implementation from being even more confusing than it already is.
struct ChannelMonitorChainListener {
    listener: ChannelMonitorListenerType,
    funding_outpoint: OutPoint,
}

impl ChannelMonitorChainListener {
    fn new(
        channel_monitor: ChannelMonitorType,
        broadcaster: Arc<BroadcasterType>,
        fee_estimator: Arc<FeeEstimatorType>,
        logger: LexeTracingLogger,
    ) -> Self {
        let (funding_outpoint, _script) = channel_monitor.get_funding_txo();

        // This tuple is ChannelMonitorListenerType, which LDK impls Listen for
        let listener = (channel_monitor, broadcaster, fee_estimator, logger);

        Self {
            listener,
            funding_outpoint,
        }
    }

    /// Consumes self, returning the inner ChannelMonitor and funding outpoint.
    fn into_monitor_and_outpoint(self) -> (ChannelMonitorType, OutPoint) {
        let (channel_monitor, _broadcaster, _fee_est, _log) = self.listener;
        (channel_monitor, self.funding_outpoint)
    }
}
