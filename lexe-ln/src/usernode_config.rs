//! The usernode's LDK `UserConfig`, shared with the recovery node so that
//! both operate on channels with identical parameters.

use lexe_common::constants;
use lightning::{
    ln::channelmanager::MIN_CLTV_EXPIRY_DELTA,
    util::config::{
        ChannelConfig, ChannelHandshakeConfig, ChannelHandshakeLimits,
        MaxDustHTLCExposure, UserConfig,
    },
};

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

pub const fn user_config() -> UserConfig {
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
        // Zero-fee commitments are required for zero-reserve user channels.
        //
        // Force-closes rely on external fee-bump funds and TRUC/P2A relay.
        // For a force-close tx to reach miners and get confirmed, zero-fee
        // commitment channels require a path from your Bitcoin node to miners
        // that relays TRUC transactions (BIP 431), P2A outputs, and Ephemeral
        // Dust. Currently, only nodes running Bitcoin Core v29 and above relay
        // transactions with these features.
        negotiate_anchor_zero_fee_commitments: true,
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
