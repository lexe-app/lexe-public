use std::{collections::HashMap, ops::Deref, sync::Arc, time::SystemTime};

use anyhow::Context;
use bitcoin::BlockHash;
use common::{constants, ln::network::LxNetwork};
use lexe_ln::{
    alias::{BroadcasterType, FeeEstimatorType, MessageRouterType, RouterType},
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
use tracing::{debug, info, warn};

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

// This fn prevents the rest of the crate from instantiating configs directly.
pub(crate) fn get_config() -> Arc<UserConfig> {
    Arc::new(user_config())
}

const fn user_config() -> UserConfig {
    UserConfig {
        channel_handshake_config: channel_handshake_config(),
        channel_handshake_limits: channel_handshake_limits(),
        channel_config: channel_config(),

        // Do not accept any HTLC forwarding risks
        accept_forwards_to_priv_channels: false,
        // We accept inbound channels, but only those initiated by the LSP.
        accept_inbound_channels: true,
        // Manually accepting inbound channels is required for zeroconf, and for
        // checking that the inbound channel was initiated by Lexe's LSP.
        // See Event::OpenChannelRequest in the event handler.
        //
        // NOTE(zeroconf): Zeroconf channels allow you to receive Lightning
        // payments immediately (without having to wait for
        // confirmations) in the case that you do not yet have a channel
        // open with Lexe's LSP. The channel is immediately usable,
        // meaning you can use those zeroconf funds to then make
        // an outbound payment of your own. However, zeroconf exposes you to the
        // following risks and caveats:
        //
        // - If you are a merchant, theoretically Lexe could pretend to be your
        //   customer and purchase a good or service from you using a zeroconf
        //   channel. If you render the good or service before the zeroconf
        //   channel has gotten least a few confirmations (3-6), Lexe could
        //   double-spend the funding transaction, defrauding you of your
        //   payment. If you do not trust Lexe not to double-spend the funding
        //   tx, do not render any goods or services until the payment has been
        //   'finalized' in the Lexe app, or disable zeroconf entirely in your
        //   app settings.
        // - If you are using Lexe to accept Lightning tips, theoretically Lexe
        //   could siphon off these tips by (1) extending the Lightning payment
        //   to your node over a zeroconf channel, (2) collecting its payment
        //   from its previous hop, then (3) defrauding your node by
        //   double-spending the funding tx. If you do not trust Lexe not to do
        //   this, do not enable zeroconf channels.
        //
        // TODO(max): Expose payments and channel balances to the user as
        // pending / finalized depending on channel confirmation status.
        // TODO(max): Expose an option for enabling / disabling zeroconf.
        // TODO(max): Convert these notes into a blog post or help article of
        // some kind which is accessible from the users' mobile app.
        // TODO(max): Add more notes corresponding to the results of current
        // research on zeroconf channels in Nuclino
        manually_accept_inbound_channels: true,
        // The node has no need to intercept HTLCs
        accept_intercept_htlcs: false,
        // For now, no need to manually pay BOLT 12 invoices when received.
        manually_handle_bolt12_invoices: false,
    }
}

const fn channel_handshake_config() -> ChannelHandshakeConfig {
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
        // Setting this to 100 minimizes the difference between the LSP's
        // `outbound_capacity` and `next_outbound_htlc_limit`.
        max_inbound_htlc_value_in_flight_percent_of_channel: 100,
        // Attempt to use better privacy.
        negotiate_scid_privacy: true,
        // TODO(max): Support anchor outputs. NOTE that as part of this we'll
        // need to ensure that `manually_accept_inbound_channels == true` so
        // that we can check that we have a sufficient wallet balance to cover
        // the fees for all existing and future channels.
        negotiate_anchors_zero_fee_htlc_tx: false,
        // User<->LSP channels are private. People route to us via a route hop
        // hint in the invoice.
        announce_for_forwarding: false,
        // The additional 'security' provided by this setting is pointless.
        // Also, we want to be able to sweep all funds to an address specified
        // at the time of channel close, instead of committing upfront.
        commit_upfront_shutdown_pubkey: false,
        // See docs on the const
        their_channel_reserve_proportional_millionths:
            constants::LSP_RESERVE_PROP_PPM,
    }
}

const fn channel_handshake_limits() -> ChannelHandshakeLimits {
    ChannelHandshakeLimits {
        // Force an incoming channel (from the LSP) to match the value we set
        // for `ChannelHandshakeConfig::announce_for_forwarding` (which is
        // false)
        force_announced_channel_preference: true,
        // The maximum # of blocks we're willing to wait to reclaim our funds in
        // the case of a unilateral close initiated by us. See doc comment.
        their_to_self_delay: MAXIMUM_TIME_TO_RECLAIM_FUNDS,
        // The maximum total channel value (our balance + their balance) that
        // we'll accept for a new inbound channel.
        // The current LDK default was too low (0.16_777_216 BTC, the pre-wumbo
        // channel maximum).
        max_funding_satoshis: constants::CHANNEL_MAX_FUNDING_SATS as u64,
        // Use LDK defaults for everything else. We can't use Default::default()
        // in a const, but it's better to explicitly specify the values anyway.
        min_funding_satoshis: 0,
        max_htlc_minimum_msat: u64::MAX,
        min_max_htlc_value_in_flight_msat: 0,
        max_channel_reserve_satoshis: u64::MAX,
        min_max_accepted_htlcs: 0,
        trust_own_funding_0conf: true,
        max_minimum_depth: 144,
    }
}

const fn channel_config() -> ChannelConfig {
    ChannelConfig {
        // This allows the user node to pay the on-chain fees for JIT channel
        // opens.
        accept_underpaying_htlcs: true,
        // (proportional fee) We do not forward anything so this can be 0
        forwarding_fee_proportional_millionths: 0,
        // (base fee) We do not forward anything so this can be 0
        forwarding_fee_base_msat: 0,
        // We do not forward anything so this can be the minimum
        cltv_expiry_delta: MIN_CLTV_EXPIRY_DELTA,
        // NOTE: Increases `ChannelDetails::next_outbound_htlc_minimum_msat`
        // if this is set too low, causing small payments to fail to route.
        // Current setting: 100k sats
        max_dust_htlc_exposure: MaxDustHTLCExposure::FixedLimitMsat(
            100_000_000,
        ),
        // Pay up to 1000 sats ($1 assuming $100K per BTC) to avoid waiting up
        // to `their_to_self_delay` time (currently set to ~1 day) in the case
        // of a unilateral close initiated by us. In practice our LSP should
        // always be online so this should rarely, if ever, be paid.
        force_close_avoidance_max_fee_satoshis:
            constants::FORCE_CLOSE_AVOIDANCE_MAX_FEE_SATS,
    }
}

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
    pub(crate) fn init(
        network: LxNetwork,
        config: UserConfig,
        maybe_manager: Option<(BlockHash, ChannelManagerType)>,
        keys_manager: Arc<LexeKeysManager>,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        router: Arc<RouterType>,
        message_router: Arc<MessageRouterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Self> {
        debug!("Initializing channel manager");

        let (blockhash, inner, label) = match maybe_manager {
            Some((blockhash, mgr)) => (blockhash, mgr, "persisted"),
            None => {
                // We're starting a fresh node.
                // Use the genesis block as the current best block.
                let genesis_hash = network.genesis_block_hash();
                let genesis_height = 0;
                let best_block = BestBlock::new(genesis_hash, genesis_height);
                let chain_params = ChainParameters {
                    network: network.to_bitcoin(),
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
                    message_router,
                    logger,
                    keys_manager.clone(),
                    keys_manager.clone(),
                    keys_manager,
                    config,
                    chain_params,
                    current_timestamp_secs,
                );
                (genesis_hash, inner, "fresh")
            }
        };
        info!(%blockhash, "Loaded {label} channel manager");

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
