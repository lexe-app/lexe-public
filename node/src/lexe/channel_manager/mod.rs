use std::ops::Deref;
use std::sync::Arc;

use anyhow::Context;
use bitcoin::BlockHash;
use lightning::chain::BestBlock;
use lightning::ln::channelmanager;
use lightning::ln::channelmanager::ChainParameters;
use lightning::util::config::UserConfig;

use crate::cli::StartCommand;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::persister::LexePersister;
use crate::types::{
    BlockSourceType, BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorType, FeeEstimatorType,
};

mod types;

pub use types::*;

/// An Arc is held internally, so it is fine to clone directly.
#[derive(Clone)]
pub struct LexeChannelManager(Arc<ChannelManagerType>);

impl Deref for LexeChannelManager {
    type Target = ChannelManagerType;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl LexeChannelManager {
    #[allow(clippy::too_many_arguments)]
    pub async fn init(
        args: &StartCommand,
        persister: &LexePersister,
        block_source: &BlockSourceType,
        restarting_node: &mut bool,
        channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
        keys_manager: LexeKeysManager,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<(BlockHash, Self)> {
        println!("Initializing channel manager");
        let mut user_config = UserConfig::default();
        user_config
            .peer_channel_config_limits
            .force_announced_channel_preference = false;
        let inner_opt = persister
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
        let (channel_manager_blockhash, inner) = match inner_opt {
            Some((blockhash, mgr)) => (blockhash, mgr),
            None => {
                // We're starting a fresh node.
                *restarting_node = false;
                let getinfo_resp = block_source.get_blockchain_info().await;

                let chain_params = ChainParameters {
                    network: args.network.into_inner(),
                    best_block: BestBlock::new(
                        getinfo_resp.latest_blockhash,
                        getinfo_resp.latest_height as u32,
                    ),
                };
                let fresh_inner = channelmanager::ChannelManager::new(
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    logger,
                    keys_manager,
                    user_config,
                    chain_params,
                );
                (getinfo_resp.latest_blockhash, fresh_inner)
            }
        };

        let channel_manager = Self(Arc::new(inner));

        println!("    channel manager done.");
        Ok((channel_manager_blockhash, channel_manager))
    }
}
