use std::{collections::HashMap, ops::Deref, sync::Arc, time::SystemTime};

use anyhow::Context;
use lexe_common::{constants, ln::network::Network};
use lexe_ln::{
    alias::{BroadcasterType, FeeEstimatorType, MessageRouterType, RouterType},
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
};
use lightning::{
    chain::BlockLocator,
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
        //
        // LDK requires every inbound channel to be accepted manually, which we
        // need anyway for zeroconf and to check that the channel was initiated
        // by Lexe's LSP. See Event::OpenChannelRequest in the event handler.
        accept_inbound_channels: true,
        // TODO(phlip9): splicing needs testing.
        reject_inbound_splices: true,
        // The node has no need to intercept HTLCs
        htlc_interception_flags: 0,
        // For now, no need to manually pay BOLT 12 invoices when received.
        manually_handle_bolt12_invoices: false,
        // TODO(phlip9): support splicing/dual-funded channels
        enable_dual_funded_channels: false,
        // This feature enables the node to hold onto HTLCs until its peer is
        // online again. User nodes are not routing nodes, so this is not
        // relevant.
        enable_htlc_hold: false,
        // This feature would allow user nodes to pay a `StaticInvoice` to
        // another "often-offline" recipient by having the LSP hold the invoice
        // for us.
        // TODO(phlip9): potentially relevant, would need LSP to support
        // `enable_htlc_hold`.
        hold_outbound_htlcs_at_next_hop: false,
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
        // `outbound_capacity` and `next_outbound_htlc_limit`. Our channels are
        // unannounced, but set both so the limit doesn't depend on that.
        announced_channel_max_inbound_htlc_value_in_flight_percentage: 100,
        unannounced_channel_max_inbound_htlc_value_in_flight_percentage: 100,
        // Attempt to use better privacy.
        negotiate_scid_privacy: true,
        // TODO(max): Support anchor outputs.
        negotiate_anchors_zero_fee_htlc_tx: false,
        // If true, we'll attempt to negotiate zero-fee commitments for all
        // future channels.
        //
        // For a force-close transaction to reach miners and get confirmed,
        // zero-fee commitment channels require a path from your Bitcoin node to
        // miners that relays TRUC transactions (BIP 431), P2A outputs,
        // and Ephemeral Dust. Currently, only nodes running Bitcoin
        // Core v29 and above relay transactions with these features.
        //
        // TODO(phlip9): needs testing.
        negotiate_anchor_zero_fee_commitments: false,
        // User<->LSP channels are private. People route to us via a route hop
        // hint in the invoice.
        announce_for_forwarding: false,
        // The additional 'security' provided by this setting is pointless.
        // Also, we want to be able to sweep all funds to an address specified
        // at the time of channel close, instead of committing upfront.
        //
        // If we change this to `true`, we may need to reevaluate
        // `LexeKeysManager::get_shutdown_scriptpubkey`.
        commit_upfront_shutdown_pubkey: false,
        // See docs on the const
        their_channel_reserve_proportional_millionths:
            constants::LSP_RESERVE_PROPORTION.to_u32(),
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
        // LDK always adds this to the funder's coop-close max_fee.
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
