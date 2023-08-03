use std::{ops::Deref, sync::Arc, time::SystemTime};

use anyhow::Context;
use bitcoin::{blockdata::constants, BlockHash};
use common::cli::Network;
use lexe_ln::{
    alias::{BroadcasterType, FeeEstimatorType, RouterType},
    esplora::HIGH_PRIORITY_SATS_PER_KW,
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
};
use lightning::{
    chain::BestBlock,
    ln::channelmanager::{
        ChainParameters, ChannelManager, MIN_CLTV_EXPIRY_DELTA,
    },
    util::config::{
        ChannelConfig, ChannelHandshakeConfig, ChannelHandshakeLimits,
        MaxDustHTLCExposure, UserConfig,
    },
};
use tracing::{debug, info};

use crate::alias::{ChainMonitorType, ChannelManagerType};

/// NOTE: Important security parameter!! This is specified in # of blocks.
///
/// Since the mobile client verifies the latest security report every time the
/// mobile client boots, and the security report checks the blockchain for
/// channel close transactions, the user can guarantee the security of their
/// funds by opening their app at least once every (this parameter).
///
/// This value can be decreased if the mobile client has a recurring task to
/// verify the security report e.g. once every day. This appears to be possible
/// with Android's `JobScheduler`, but more difficult (or not possible) on iOS.
///
/// The minimum and maximum values allowed by LDK are 144 blocks (1
/// day, i.e.[`BREAKDOWN_TIMEOUT`]) and 2016 blocks (two weeks) respectively.
///
/// [`BREAKDOWN_TIMEOUT`]: lightning::ln::channelmanager::BREAKDOWN_TIMEOUT
const TIME_TO_CONTEST_FRAUDULENT_CLOSES: u16 = 6 * 24 * 7; // 7 days

/// The inverse of [`TIME_TO_CONTEST_FRAUDULENT_CLOSES`], specified in blocks.
/// Defines the maximum number of blocks we're willing to wait to reclaim our
/// funds in the case of a unilateral close initiated by us.
///
/// NOTE: If this value is too low, channel negotiation with the LSP will fail.
const MAXIMUM_TIME_TO_RECLAIM_FUNDS: u16 = 6 * 24 * 4; // four days

pub const USER_CONFIG: UserConfig = UserConfig {
    channel_handshake_config: CHANNEL_HANDSHAKE_CONFIG,
    channel_handshake_limits: CHANNEL_HANDSHAKE_LIMITS,
    channel_config: CHANNEL_CONFIG,

    // Do not accept any HTLC forwarding risks
    accept_forwards_to_priv_channels: false,
    // We accept inbound channels, but only those initiated by the LSP.
    accept_inbound_channels: true,
    // Manually accepting inbound channels is required for zeroconf, and for
    // checking that the inbound channel was initiated by Lexe's LSP.
    // See Event::OpenChannelRequest in the event handler.
    //
    // NOTE(zeroconf): Zeroconf channels allow you to receive Lightning payments
    // immediately (without having to wait for confirmations) in the case that
    // you do not yet have a channel open with Lexe's LSP. The channel is
    // immediately usable, meaning you can use those zeroconf funds to then make
    // an outbound payment of your own. However, zeroconf exposes you to the
    // following risks and caveats:
    //
    // - If you are a merchant, theoretically Lexe could pretend to be your
    //   customer and purchase a good or service from you using a zeroconf
    //   channel. If you render the good or service before the zeroconf channel
    //   has gotten least a few confirmations (3-6), Lexe could double-spend the
    //   funding transaction, defrauding you of your payment. If you do not
    //   trust Lexe not to double-spend the funding tx, do not render any goods
    //   or services until the payment has been 'finalized' in the Lexe app, or
    //   disable zeroconf entirely in your app settings.
    // - If you are using Lexe to accept Lightning tips, theoretically Lexe
    //   could siphon off these tips by (1) extending the Lightning payment to
    //   your node over a zeroconf channel, (2) collecting its payment from its
    //   previous hop, then (3) defrauding your node by double-spending the
    //   funding tx. If you do not trust Lexe not to do this, do not enable
    //   zeroconf channels.
    //
    // TODO(max): Expose payments and channel balances to the user as pending /
    // finalized depending on channel confirmation status.
    // TODO(max): Expose an option for enabling / disabling zeroconf.
    // TODO(max): Convert these notes into a blog post or help article of some
    // kind which is accessible from the users' mobile app.
    // TODO(max): Add more notes corresponding to the results of current
    // research on zeroconf channels in Nuclino
    manually_accept_inbound_channels: true,
    // The node has no need to intercept HTLCs
    accept_intercept_htlcs: false,
    // Allow receiving keysend payments composed of multiple parts.
    accept_mpp_keysend: true,
};

const CHANNEL_HANDSHAKE_CONFIG: ChannelHandshakeConfig =
    ChannelHandshakeConfig {
        // Wait 3 confirmations for channels to be considered locked-in.
        minimum_depth: 3,
        // Require the channel counterparty (Lexe's LSPs) to wait <this param>
        // to claim funds in the case of a unilateral close. Specified
        // in # of blocks.
        our_to_self_delay: TIME_TO_CONTEST_FRAUDULENT_CLOSES,
        // Allow extremely small HTLCs
        our_htlc_minimum_msat: 1,
        // LDK's default limit on the number of inflight inbound HTLCs.
        our_max_accepted_htlcs: 50,
        // Allow up to 100% of our funds to be encumbered in inbound HTLCS.
        max_inbound_htlc_value_in_flight_percent_of_channel: 100,
        // Attempt to use better privacy.
        negotiate_scid_privacy: true,
        // TODO(max): Support anchor outputs. NOTE that as part of this we'll
        // need to ensure that `manually_accept_inbound_channels == true` so
        // that we can check that we have a sufficient wallet balance to cover
        // the fees for all existing and future channels.
        negotiate_anchors_zero_fee_htlc_tx: false,
        // Publically announce our channels
        // TODO: Is there a way to *not* publicly announce our channel, but
        // still be able to complete a channel negatiation with the LSP?
        announced_channel: true,
        // The additional 'security' provided by this setting is pointless.
        // Also, we want to be able to sweep all funds to an address specified
        // at the time of channel close, instead of committing upfront.
        commit_upfront_shutdown_pubkey: false,
        // The counterparty must reserve 1% of the total channel value to be
        // claimable by us on-chain in the case of a channel breach.
        their_channel_reserve_proportional_millionths: 10_000,
    };

const CHANNEL_HANDSHAKE_LIMITS: ChannelHandshakeLimits =
    ChannelHandshakeLimits {
        // Force an incoming channel (from the LSP) to match the value we set
        // for `ChannelHandshakeConfig::announced_channel` (which is false)
        force_announced_channel_preference: true,
        // The maximum # of blocks we're willing to wait to reclaim our funds in
        // the case of a unilateral close initiated by us. See doc comment.
        their_to_self_delay: MAXIMUM_TIME_TO_RECLAIM_FUNDS,
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

const CHANNEL_CONFIG: ChannelConfig = ChannelConfig {
    // This allows the user node to pay the on-chain fees for JIT channel opens.
    accept_underpaying_htlcs: true,
    // (proportional fee) We do not forward anything so this can be 0
    forwarding_fee_proportional_millionths: 0,
    // (base fee) We do not forward anything so this can be 0
    forwarding_fee_base_msat: 0,
    // We do not forward anything so this can be the minimum
    cltv_expiry_delta: MIN_CLTV_EXPIRY_DELTA,
    // LDK default
    max_dust_htlc_exposure: MaxDustHTLCExposure::FeeRateMultiplier(
        HIGH_PRIORITY_SATS_PER_KW as u64,
    ),
    // Pay up to 1000 sats (50 cents assuming $50K per BTC) to avoid waiting up
    // to `their_to_self_delay` time (currently set to ~1 day) in the case of a
    // unilateral close initiated by us. In practice our LSP should always be
    // online so this should rarely, if ever, be paid.
    force_close_avoidance_max_fee_satoshis: 1000,
};

/// An Arc is held internally, so it is fine to clone directly.
#[derive(Clone)]
pub struct NodeChannelManager(Arc<ChannelManagerType>);

impl Deref for NodeChannelManager {
    type Target = ChannelManagerType;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl NodeChannelManager {
    pub fn arc_inner(&self) -> Arc<ChannelManagerType> {
        self.0.clone()
    }

    pub(crate) fn init(
        network: Network,
        maybe_manager: Option<(BlockHash, ChannelManagerType)>,
        keys_manager: Arc<LexeKeysManager>,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        router: Arc<RouterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Self> {
        debug!("Initializing channel manager");

        let (blockhash, inner, label) = match maybe_manager {
            Some((blockhash, mgr)) => (blockhash, mgr, "persisted"),
            None => {
                // We're starting a fresh node.
                // Use the genesis block as the current best block.
                let network = network.to_inner();
                let genesis_hash =
                    constants::genesis_block(network).header.block_hash();
                let genesis_height = 0;
                let best_block = BestBlock::new(genesis_hash, genesis_height);
                let chain_params = ChainParameters {
                    network,
                    best_block,
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
                    logger,
                    keys_manager.clone(),
                    keys_manager.clone(),
                    keys_manager,
                    USER_CONFIG,
                    chain_params,
                    current_timestamp_secs,
                );
                (genesis_hash, inner, "fresh")
            }
        };
        info!(%blockhash, "Loaded {label} channel manager");

        Ok(Self(Arc::new(inner)))
    }

    // TODO: Closing a channel should delete a channel peer.
}
