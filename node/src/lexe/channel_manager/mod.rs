use std::ops::Deref;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use bitcoin::BlockHash;
use lightning::chain::BestBlock;
use lightning::ln::channelmanager::{
    ChainParameters, ChannelManager, BREAKDOWN_TIMEOUT, MIN_CLTV_EXPIRY_DELTA,
};
use lightning::util::config::{
    ChannelConfig, ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig,
};
use tracing::info;

use crate::cli::StartCommand;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::peer_manager::{ChannelPeer, LexePeerManager};
use crate::lexe::persister::LexePersister;
use crate::types::{
    BlockSourceType, BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorType, FeeEstimatorType,
};

mod types;

pub use types::*;

/// NOTE: Important security parameter!!
///
/// Since the mobile client verifies the latest security report every time the
/// mobile client boots, and the security report checks the blockchain for
/// channel close transactions, the user can guarantee the security of their
/// funds by opening their app at least once every <this parameter>.
///
/// This value can be decreased if the mobile client has a recurring task to
/// verify the security report e.g. once every day. This appears to be possible
/// with Android's `JobScheduler`, but more difficult (or not possible) on iOS.
///
/// Note that the minimum and maximum values allowed by LDK are 144 blocks (1
/// day, i.e. `BREAKDOWN_TIMEOUT`) and 2016 blocks (two weeks) respectively.
///
/// TODO: Implement security report which checks for channel closes
/// TODO: Implement recurring verification of the security report
#[cfg(all(target_env = "sgx", not(test)))]
const TIME_TO_CONTEST_FRAUDULENT_TXNS: u16 = 6 * 24 * 7;
// Use less secure parameters during development
#[cfg(any(not(target_env = "sgx"), test))]
const TIME_TO_CONTEST_FRAUDULENT_TXNS: u16 = BREAKDOWN_TIMEOUT;

pub const USER_CONFIG: UserConfig = UserConfig {
    own_channel_config: OWN_CHANNEL_CONFIG,
    peer_channel_config_limits: PEER_CHANNEL_CONFIG_LIMITS,
    channel_options: CHANNEL_OPTIONS,

    // Do not accept any HTLC forwarding risks
    accept_forwards_to_priv_channels: false,
    // We accept inbound channels, but only those initiated by the LSP.
    // TODO Verify that inbound channels were opened by the LSP
    accept_inbound_channels: true,
    // NOTE: False for now, but this will need to change to true once we
    // implemente the check that inbound channels were initiated by the LSP.
    manually_accept_inbound_channels: false,
};

const OWN_CHANNEL_CONFIG: ChannelHandshakeConfig = ChannelHandshakeConfig {
    // Wait 6 confirmations for channels to be considered locked-in.
    minimum_depth: 6,
    // Require the channel counterparty (Lexe's LSPs) to wait <this param> to
    // claim funds in the case of a unilateral close. Specified in # of blocks.
    our_to_self_delay: TIME_TO_CONTEST_FRAUDULENT_TXNS,
    // Allow extremely small HTLCs
    our_htlc_minimum_msat: 1,
    // Allow up to 100% of our funds to be encumbered in inbound HTLCS.
    max_inbound_htlc_value_in_flight_percent_of_channel: 100,
    // Attempt to use better privacy. The LSP should have this enabled.
    negotiate_scid_privacy: true,
};

const PEER_CHANNEL_CONFIG_LIMITS: ChannelHandshakeLimits =
    ChannelHandshakeLimits {
        // Force an incoming channel (from the LSP) to match the value we set
        // for `ChannelConfig::announced_channel` (which is false)
        force_announced_channel_preference: true,
        // *We* (the node) wait a maximum of 6 * 24 blocks (1 day) to reclaim
        // our funds in the case of a unilateral close initiated by us.
        their_to_self_delay: BREAKDOWN_TIMEOUT,
        // Use LDK defaults for everything else. We can't use Default::default()
        // in a const, but it's better to explicitly specify the values anyway.
        min_funding_satoshis: 0,
        max_funding_satoshis: (1 << 24) - 1, // 2^24 - 1
        max_htlc_minimum_msat: u64::MAX,
        min_max_htlc_value_in_flight_msat: 0,
        max_channel_reserve_satoshis: u64::MAX,
        min_max_accepted_htlcs: 0,
        trust_own_funding_0conf: true,
        max_minimum_depth: 144,
    };

const CHANNEL_OPTIONS: ChannelConfig = ChannelConfig {
    // (proportional fee) We do not forward anything so this can be 0
    forwarding_fee_proportional_millionths: 0,
    // (base fee) We do not forward anything so this can be 0
    forwarding_fee_base_msat: 0,
    // We do not forward anything so this can be the minimum
    cltv_expiry_delta: MIN_CLTV_EXPIRY_DELTA,
    // Do not publically announce our channels
    announced_channel: false,
    // The additional 'security' provided by setting is pointless. Additionally,
    // we do not want to commit to a `shutdown_pubkey` so that it is possible to
    // sweep all funds to an address specified at the time of channel close.
    commit_upfront_shutdown_pubkey: false,
    // LDK default
    max_dust_htlc_exposure_msat: 5_000_000,
    // Pay up to 1000 sats (50 cents assuming $50K per BTC) to avoid waiting up
    // to `their_to_self_delay` time (currently set to ~1 day) in the case of a
    // unilateral close initiated by us. In practice our LSP should always be
    // online so this should rarely, if ever, be paid.
    force_close_avoidance_max_fee_satoshis: 1000,
};

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
    // TODO: Review this function and clean up accordingly
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
        let inner_opt = persister
            .read_channel_manager(
                channel_monitors,
                keys_manager.clone(),
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                logger.clone(),
            )
            .await
            .context("Could not read ChannelManager from DB")?;
        let (channel_manager_blockhash, inner) = match inner_opt {
            Some((blockhash, mgr)) => (blockhash, mgr),
            None => {
                // We're starting a fresh node.
                *restarting_node = false;
                let blockchain_info = block_source.get_blockchain_info().await;

                let best_block = BestBlock::new(
                    blockchain_info.latest_blockhash,
                    blockchain_info.latest_height as u32,
                );
                let chain_params = ChainParameters {
                    network: args.network.into_inner(),
                    best_block,
                };
                let inner = ChannelManager::new(
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    logger,
                    keys_manager,
                    USER_CONFIG,
                    chain_params,
                );
                (blockchain_info.latest_blockhash, inner)
            }
        };

        let channel_manager = Self(Arc::new(inner));

        println!("    channel manager done.");
        Ok((channel_manager_blockhash, channel_manager))
    }

    /// Handles the full logic of opening a channel, including connecting to the
    /// peer, creating the channel, and persisting the newly created channel.
    pub async fn open_channel(
        &self,
        peer_manager: &LexePeerManager,
        persister: &LexePersister,
        channel_peer: ChannelPeer,
        channel_value_sat: u64,
    ) -> anyhow::Result<()> {
        // Make sure that we're connected to the channel peer
        peer_manager
            .connect_peer_if_necessary(channel_peer.clone())
            .await
            .context("Could not connect to peer")?;

        // Create the channel
        let user_channel_id = 1; // Not important, just use a default value
        let push_msat = 0; // No need for this yet
        self.0
            .create_channel(
                channel_peer.pubkey,
                channel_value_sat,
                push_msat,
                user_channel_id,
                Some(USER_CONFIG),
            )
            // LDK's APIError impls Debug but not Error
            .map_err(|e| anyhow!("{:?}", e))
            .context("Could not create channel")?;

        // Persist the channel
        persister
            .persist_channel_peer(channel_peer.clone())
            .await
            .context("Could not persist channel peer")?;

        info!("Successfully opened channel with {}", channel_peer);

        Ok(())
    }
}
