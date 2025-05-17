use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

use anyhow::Context;
use byte_array::ByteArray;
use lexe_std::Apply;
use lightning::{
    chain::transaction::OutPoint,
    ln::{channel_state::ChannelDetails, types::ChannelId},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    api::user::{NodePk, Scid},
    ln::{amount::Amount, hashes::LxTxid},
    rng::{RngCore, RngExt},
    serde_helpers::hexstr_or_bytes,
};

/// A newtype for [`lightning::ln::types::ChannelId`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxChannelId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

byte_array::impl_byte_array!(LxChannelId, 32);
byte_array::impl_fromstr_from_hexstr!(LxChannelId);
byte_array::impl_debug_display_as_hex!(LxChannelId);

impl From<ChannelId> for LxChannelId {
    fn from(cid: ChannelId) -> Self {
        Self(cid.0)
    }
}
impl From<LxChannelId> for ChannelId {
    fn from(cid: LxChannelId) -> Self {
        Self(cid.0)
    }
}

/// See: [`lightning::ln::channel_state::ChannelDetails::user_channel_id`]
///
/// The user channel id lets us consistently identify a channel through its
/// whole lifecycle.
///
/// The main issue is that we don't know the [`LxChannelId`] until we've
/// actually talked to the remote node and agreed to open a channel. The second
/// issue is that we can't easily observe and correlate any errors from channel
/// negotiation beyond some basic checks before we send any messages.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxUserChannelId(#[serde(with = "hexstr_or_bytes")] pub [u8; 16]);

impl LxUserChannelId {
    #[inline]
    pub fn to_u128(self) -> u128 {
        u128::from_le_bytes(self.0)
    }

    pub fn from_rng<R: RngCore>(rng: &mut R) -> Self {
        Self(rng.gen_bytes())
    }

    pub fn derive_temporary_channel_id(&self) -> LxChannelId {
        LxChannelId(sha256::digest(&self.0).into_inner())
    }
}

byte_array::impl_byte_array!(LxUserChannelId, 16);
byte_array::impl_fromstr_from_hexstr!(LxUserChannelId);
byte_array::impl_debug_display_as_hex!(LxUserChannelId);

impl From<u128> for LxUserChannelId {
    fn from(value: u128) -> Self {
        Self(value.to_le_bytes())
    }
}

impl From<LxUserChannelId> for u128 {
    fn from(value: LxUserChannelId) -> Self {
        value.to_u128()
    }
}

/// A newtype for LDK's [`ChannelDetails`] which implements the [`serde`]
/// traits, flattens nested structure, and contains only the fields Lexe needs.
#[derive(Debug, Serialize, Deserialize)]
pub struct LxChannelDetails {
    // --- Basic info --- //
    pub channel_id: LxChannelId,
    pub user_channel_id: LxUserChannelId,
    /// The position of the funding transaction in the chain.
    /// - Used as a short identifier in many places.
    /// - [`None`] if the funding tx hasn't been confirmed.
    /// - NOTE: If `inbound_scid_alias` is present, it must be used for
    ///   invoices and inbound payments instead of this.
    // Introduced in node-v0.6.21, lsp-v0.6.37
    pub scid: Option<Scid>,
    /// The result of [`ChannelDetails::get_inbound_payment_scid`].
    /// NOTE: Use this for inbound payments and route hints instead of
    /// [`Self::scid`]. See [`ChannelDetails::inbound_scid_alias`] for details.
    // Introduced in node-v0.6.21, lsp-v0.6.37
    pub inbound_payment_scid: Option<Scid>,
    /// The result of [`ChannelDetails::get_outbound_payment_scid`].
    /// NOTE: This should be used in `Route`s to describe the first hop and
    /// when we send or forward a payment outbound over this channel.
    /// See [`ChannelDetails::outbound_scid_alias`] for details.
    // Introduced in node-v0.6.21, lsp-v0.6.37
    pub outbound_payment_scid: Option<Scid>,
    pub funding_txo: Option<LxOutPoint>,
    // Introduced in node-v0.6.16, lsp-v0.6.32
    pub counterparty_alias: Option<String>,
    pub counterparty_node_id: NodePk,
    pub channel_value: Amount,
    /// The portion of our balance that our counterparty forces us to keep in
    /// the channel so they can punish us if we try to cheat. Unspendable.
    pub punishment_reserve: Amount,
    /// The number of blocks we'll need to wait to claim our funds if we
    /// initiate a channel close. Is [`None`] if the channel is outbound and
    /// hasn't yet been accepted by our counterparty.
    pub force_close_spend_delay: Option<u16>,
    // This field was `is_public` prior to LDK v0.0.124
    pub is_announced: bool,
    pub is_outbound: bool,
    /// (1) channel has been confirmed
    /// (2) `channel_ready` messages have been exchanged
    /// (3) channel is not currently being shut down
    pub is_ready: bool,
    /// (1) `is_ready`
    /// (2) we are (p2p) connected to our counterparty
    pub is_usable: bool,

    // --- Our balance --- //
    /// Our total balance. "The amount we would get if we close the channel*"
    /// *if all pending inbound HTLCs failed and on-chain fees were 0
    ///
    /// Use this for displaying our "current funds".
    pub our_balance: Amount,
    /// Is: `balance - punishment_reserve - pending_outbound_htlcs`
    ///
    /// Use this as an approximate measurement of liquidity, e.g. in graphics.
    pub outbound_capacity: Amount,
    /// Roughly: `outbound_capacity`, but accounting for all additional
    /// protocol limits like commitment tx fees, dust limits, and
    /// counterparty constraints.
    ///
    /// Use this for routing, including determining the maximum size of the
    /// next individual Lightning payment sent over this channel, or
    /// determining how much can be sent in this channel's shard of a
    /// multi-path payment.
    pub next_outbound_htlc_limit: Amount,

    // --- Their balance --- //
    pub their_balance: Amount,
    /// Approximately how much inbound capacity is available to us.
    ///
    /// Due to in-flight HTLCs, feerates, dust limits, etc... we cannot
    /// receive exactly this value (likely a 1k-2k sats lower).
    pub inbound_capacity: Amount,

    // --- Fees and CLTV --- //
    // These values may change at runtime.
    /// Our base fee for payments forwarded outbound over this channel.
    pub our_base_fee: Amount,
    /// Our proportional fee for payments forwarded outbound over this channel.
    /// Represented as a decimal (e.g. a value of `0.01` means 1%)
    pub our_prop_fee: Decimal,
    /// The minimum difference in `cltv_expiry` that we enforce for HTLCs
    /// forwarded outbound over this channel.
    pub our_cltv_expiry_delta: u16,
    /// Their base fee for payments forwarded inbound over this channel.
    pub their_base_fee: Option<Amount>,
    /// Their proportional fee for payments forwarded inbound over this
    /// channel. Represented as a decimal (e.g. a value of `0.01` means 1%)
    pub their_prop_fee: Option<Decimal>,
    /// The minimum difference in `cltv_expiry` our counterparty enforces for
    /// HTLCs forwarded inbound over this channel.
    pub their_cltv_expiry_delta: Option<u16>,

    // --- HTLC limits --- //
    /// The smallest inbound HTLC we will accept. Generally determined by our
    /// [`ChannelHandshakeConfig::our_htlc_minimum_msat`](lightning::util::config::ChannelHandshakeConfig).
    pub inbound_htlc_minimum: Amount,
    /// The largest inbound HTLC we will accept. This is bounded above by
    /// [`Self::channel_value`] (NOT [`Self::inbound_capacity`]).
    pub inbound_htlc_maximum: Option<Amount>,
    /// The smallest outbound HTLC our counterparty will accept. Assuming the
    /// counterparty is a Lexe user or Lexe's LSP, this is determined by their
    /// [`ChannelHandshakeConfig::our_htlc_minimum_msat`](lightning::util::config::ChannelHandshakeConfig).
    pub outbound_htlc_minimum: Option<Amount>,
    /// The largest outbound HTLC our counterparty will accept. Assuming the
    /// counterparty is a Lexe user or Lexe's LSP, this appears to be bounded
    /// above by [`Self::channel_value`] (NOT [`Self::outbound_capacity`]).
    pub outbound_htlc_maximum: Option<Amount>,

    // --- Features of interest that our counterparty supports --- //
    // NOTE: In order to use these features, we must enable them as well.
    pub cpty_supports_basic_mpp: bool,
    pub cpty_supports_onion_messages: bool,
    pub cpty_supports_wumbo: bool,
    pub cpty_supports_zero_conf: bool,
}

impl LxChannelDetails {
    /// Construct a [`LxChannelDetails`] from a LDK [`ChannelDetails`] and
    /// other info.
    ///
    /// - The balance should be from [`ChannelMonitor::get_claimable_balances`];
    ///   not to be confused with [`ChainMonitor::get_claimable_balances`].
    ///
    /// [`ChannelMonitor::get_claimable_balances`]: lightning::chain::channelmonitor::ChannelMonitor::get_claimable_balances
    /// [`ChainMonitor::get_claimable_balances`]: lightning::chain::chainmonitor::ChainMonitor::get_claimable_balances
    pub fn from_ldk(
        details: ChannelDetails,
        our_balance: Amount,
        counterparty_alias: Option<String>,
    ) -> anyhow::Result<Self> {
        let inbound_payment_scid = details.get_inbound_payment_scid().map(Scid);
        let outbound_payment_scid =
            details.get_outbound_payment_scid().map(Scid);

        // This destructuring makes clear which fields we *aren't* using,
        // in case we want to include more fields in the future.
        let ChannelDetails {
            channel_id,
            counterparty,
            funding_txo,
            channel_type: _,
            short_channel_id,
            outbound_scid_alias: _,
            inbound_scid_alias: _,
            channel_value_satoshis,
            unspendable_punishment_reserve,
            user_channel_id,
            feerate_sat_per_1000_weight: _,
            outbound_capacity_msat,
            next_outbound_htlc_limit_msat,
            next_outbound_htlc_minimum_msat: _,
            inbound_capacity_msat,
            confirmations_required: _,
            confirmations: _,
            force_close_spend_delay,
            is_outbound,
            is_channel_ready,
            channel_shutdown_state: _,
            is_usable,
            is_announced,
            inbound_htlc_minimum_msat,
            inbound_htlc_maximum_msat,
            config,
            pending_inbound_htlcs: _,
            pending_outbound_htlcs: _,
        } = details;

        let channel_id = LxChannelId::from(channel_id);
        let user_channel_id = LxUserChannelId::from(user_channel_id);
        let scid = short_channel_id.map(Scid);
        let funding_txo = funding_txo.map(LxOutPoint::from);
        let counterparty_node_id = NodePk(counterparty.node_id);
        let channel_value = Amount::try_from_sats_u64(channel_value_satoshis)
            .context("Channel value overflow")?;
        let punishment_reserve = unspendable_punishment_reserve
            .unwrap_or(0)
            .apply(Amount::try_from_sats_u64)
            .context("Punishment reserve overflow")?;
        let is_ready = is_channel_ready;

        let outbound_capacity = Amount::from_msat(outbound_capacity_msat);
        let next_outbound_htlc_limit =
            Amount::from_msat(next_outbound_htlc_limit_msat);

        let their_balance = channel_value
            .checked_sub(our_balance)
            .context("Our balance was higher than the total channel value")?;
        let inbound_capacity = Amount::from_msat(inbound_capacity_msat);

        let config = config
            // Only None prior to LDK 0.0.109
            .context("Missing config")?;
        let one_million = dec!(1_000_000);
        let our_base_fee = config
            .forwarding_fee_base_msat
            .apply(u64::from)
            .apply(Amount::from_msat);
        let our_prop_fee =
            Decimal::from(config.forwarding_fee_proportional_millionths)
                / one_million;
        let our_cltv_expiry_delta = config.cltv_expiry_delta;
        let their_base_fee =
            counterparty.forwarding_info.as_ref().map(|info| {
                info.fee_base_msat.apply(u64::from).apply(Amount::from_msat)
            });
        let their_prop_fee =
            counterparty.forwarding_info.as_ref().map(|info| {
                Decimal::from(info.fee_proportional_millionths) / one_million
            });
        let their_cltv_expiry_delta = counterparty
            .forwarding_info
            .as_ref()
            .map(|info| info.cltv_expiry_delta);

        let inbound_htlc_minimum = inbound_htlc_minimum_msat
            // Only None prior to LDK 0.0.107
            .context("Missing inbound_htlc_minimum_msat")?
            .apply(Amount::from_msat);
        let inbound_htlc_maximum =
            inbound_htlc_maximum_msat.map(Amount::from_msat);
        let outbound_htlc_minimum = counterparty
            .outbound_htlc_minimum_msat
            .map(Amount::from_msat);
        let outbound_htlc_maximum = counterparty
            .outbound_htlc_maximum_msat
            .map(Amount::from_msat);

        let cpty_supports_basic_mpp =
            counterparty.features.supports_basic_mpp();
        let cpty_supports_onion_messages =
            counterparty.features.supports_onion_messages();
        let cpty_supports_wumbo = counterparty.features.supports_wumbo();
        let cpty_supports_zero_conf =
            counterparty.features.supports_zero_conf();

        Ok(Self {
            channel_id,
            user_channel_id,
            scid,
            inbound_payment_scid,
            outbound_payment_scid,
            funding_txo,
            counterparty_alias,
            counterparty_node_id,
            channel_value,
            punishment_reserve,
            force_close_spend_delay,
            is_announced,
            is_outbound,
            is_ready,
            is_usable,
            our_balance,
            outbound_capacity,
            next_outbound_htlc_limit,

            their_balance,
            inbound_capacity,

            our_base_fee,
            our_prop_fee,
            our_cltv_expiry_delta,
            their_base_fee,
            their_prop_fee,
            their_cltv_expiry_delta,

            inbound_htlc_minimum,
            inbound_htlc_maximum,
            outbound_htlc_minimum,
            outbound_htlc_maximum,

            cpty_supports_basic_mpp,
            cpty_supports_onion_messages,
            cpty_supports_wumbo,
            cpty_supports_zero_conf,
        })
    }
}

/// A newtype for [`OutPoint`] that provides [`FromStr`] / [`Display`] impls.
///
/// Since the persister relies on the string representation to identify
/// channels, having a newtype (instead of upstreaming these impls to LDK)
/// ensures that the serialization scheme does not change from beneath us.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxOutPoint {
    pub txid: LxTxid,
    pub index: u16,
}

impl From<OutPoint> for LxOutPoint {
    fn from(op: OutPoint) -> Self {
        Self {
            txid: LxTxid(op.txid),
            index: op.index,
        }
    }
}

impl From<LxOutPoint> for OutPoint {
    fn from(op: LxOutPoint) -> Self {
        Self {
            txid: op.txid.0,
            index: op.index,
        }
    }
}

/// Deserializes from `<txid>_<index>`
impl FromStr for LxOutPoint {
    type Err = anyhow::Error;
    fn from_str(outpoint_str: &str) -> anyhow::Result<Self> {
        let mut txid_and_txindex = outpoint_str.split('_');
        let txid_str = txid_and_txindex
            .next()
            .context("Missing <txid> in <txid>_<index>")?;
        let index_str = txid_and_txindex
            .next()
            .context("Missing <index> in <txid>_<index>")?;

        let txid = LxTxid::from_str(txid_str)
            .context("Invalid txid returned from DB")?;
        let index = u16::from_str(index_str)
            .context("Could not parse index into u16")?;

        Ok(Self { txid, index })
    }
}

/// Serializes to `<txid>_<index>`
impl Display for LxOutPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", self.txid, self.index)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;
    #[test]
    fn outpoint_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxOutPoint>();
    }
}
