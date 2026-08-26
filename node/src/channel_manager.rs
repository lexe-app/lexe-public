use std::{collections::HashMap, ops::Deref, sync::Arc, time::SystemTime};

use anyhow::Context;
use lexe_common::ln::network::Network;
use lexe_ln::{
    alias::{BroadcasterType, FeeEstimatorType, MessageRouterType, RouterType},
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    usernode_config,
};
use lightning::{
    chain::BlockLocator,
    ln::channelmanager::{ChainParameters, ChannelManager},
    util::config::UserConfig,
};
use tracing::{debug, info, warn};

use crate::alias::{ChainMonitorType, ChannelManagerType};

// This fn prevents the rest of the crate from instantiating configs directly.
pub(crate) fn get_config() -> Arc<UserConfig> {
    Arc::new(usernode_config::user_config())
}

#[derive(Clone)]
pub struct NodeChannelManager(Arc<ChannelManagerType>);

impl Deref for NodeChannelManager {
    type Target = ChannelManagerType;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl NodeChannelManager {
    pub(crate) fn init(
        network: Network,
        config: UserConfig,
        maybe_manager: Option<(BlockLocator, ChannelManagerType)>,
        keys_manager: Arc<LexeKeysManager>,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: BroadcasterType,
        router: Arc<RouterType>,
        message_router: Arc<MessageRouterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Self> {
        debug!("Initializing channel manager");

        let (best_block, inner, label) = match maybe_manager {
            Some((best_block, mgr)) => (best_block, mgr, "persisted"),
            None => {
                // We're starting a fresh node.
                // Use the genesis block as the current best block.
                let network = network.to_bitcoin();
                let genesis_block = BlockLocator::from_network(network);
                let chain_params = ChainParameters {
                    network,
                    best_block: genesis_block,
                };
                let current_timestamp = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .context("Clock is before January 1st, 1970")?;
                let current_timestamp_secs =
                    u32::try_from(current_timestamp.as_secs())
                        .context("Timestamp overflowed")?;
                let inner = ChannelManager::new(
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    router,
                    message_router,
                    logger,
                    keys_manager.clone(),
                    keys_manager.clone(),
                    keys_manager,
                    config,
                    chain_params,
                    current_timestamp_secs,
                );
                (genesis_block, inner, "fresh")
            }
        };
        info!(
            blockhash = %best_block.block_hash,
            height = best_block.height,
            "Loaded {label} channel manager"
        );

        Ok(Self(Arc::new(inner)))
    }

    /// Ensures that all channels are using the most up-to-date channel config.
    pub(crate) fn check_channel_configs(&self, config: &UserConfig) {
        let channels = self.0.list_channels();
        let expected_config = config.channel_config;

        // Construct a map of `counterparty_pk -> Vec<channel_id>`
        // corresponding to channels whose configs need to be updated
        let to_update: HashMap<_, Vec<_>> = channels
            .into_iter()
            .filter(|channel| {
                let config = channel.config.expect("Launched after v0.0.109");
                config != expected_config
            })
            .fold(HashMap::new(), |mut acc, channel| {
                acc.entry(channel.counterparty.node_id)
                    .or_default()
                    .push(channel.channel_id);
                acc
            });

        // Update the configs
        for (counterparty_pk, channel_ids) in to_update {
            let result = self.0.update_channel_config(
                &counterparty_pk,
                &channel_ids,
                &expected_config,
            );
            match result {
                Ok(()) => info!("Updated channel config with LSP"),
                Err(e) => warn!("Couldn't update channel config: {e:?}"),
            }
        }
    }
}
