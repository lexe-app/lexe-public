use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::Context;
use lightning::{
    chain::transaction::OutPoint, ln::channelmanager::ChannelDetails,
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    api::NodePk,
    hex,
    hex::FromHex,
    hexstr_or_bytes,
    ln::{amount::Amount, hashes::LxTxid},
    Apply,
};

/// A newtype for [`ChannelDetails::channel_id`].
///
/// [`ChannelDetails::channel_id`]: lightning::ln::channelmanager::ChannelDetails::channel_id
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

impl FromStr for ChannelId {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self)
    }
}

impl Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(&self.0))
    }
}

/// A version of LDK's [`ChannelDetails`] containing only fields that are likely
/// to be of interest to a human, e.g. when checking up on one's channels.
/// It also uses Lexe newtypes and impls the [`serde`] traits.
#[derive(Debug, Serialize, Deserialize)]
pub struct LxChannelDetails {
    // --- Basic info --- //
    pub channel_id: ChannelId,
    pub funding_txo: Option<LxOutPoint>,
    pub counterparty_node_id: NodePk,
    pub channel_value: Amount,
    /// The portion of our balance that our counterparty forces us to keep in
    /// the channel so they can punish us if we try to cheat. Unspendable.
    pub punishment_reserve: Amount,
    /// The number of blocks we'll need to wait to claim our funds if we
    /// initiate a channel close. Is [`None`] if the channel is outbound and
    /// hasn't yet been accepted by our counterparty.
    pub force_close_spend_delay: Option<u16>,
    pub is_public: bool,
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
    pub balance: Amount,
    /// Roughly: `balance - punishment_reserve - pending_outbound_htlcs`
    ///
    /// Use this as an approximate measurement of liquidity, e.g. in graphics.
    pub outbound_capacity: Amount,
    /// Roughly: `min(outbound_capacity, per_htlc_limit)`.
    ///
    /// Use this for routing, including determining the maximum size of the
    /// next individual Lightning payment sent over this channel, or
    /// determining how much can be sent in this channel's shard of a
    /// multi-path payment.
    pub next_outbound_htlc_limit: Amount,

    // --- Their balance --- //
    pub their_balance: Amount,
    /// A lower bound on the inbound capacity available to us.
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
    /// The smallest inbound HTLC we will accept.
    pub inbound_htlc_minimum: Amount,
    /// The largest inbound HTLC we will accept.
    pub inbound_htlc_maximum: Option<Amount>,
    /// The smallest outbound HTLC our counterparty will accept.
    pub outbound_htlc_minimum: Option<Amount>,
    /// The largest outbound HTLC our counterparty will accept.
    pub outbound_htlc_maximum: Option<Amount>,

    // --- Features of interest that our counterparty supports --- //
    // NOTE: In order to use these features, we must enable them as well.
    pub cpty_supports_basic_mpp: bool,
    pub cpty_supports_onion_messages: bool,
    pub cpty_supports_wumbo: bool,
    pub cpty_supports_zero_conf: bool,
}

impl From<ChannelDetails> for LxChannelDetails {
    fn from(
        ChannelDetails {
            channel_id,
            counterparty,
            funding_txo,
            channel_value_satoshis,
            unspendable_punishment_reserve,
            inbound_htlc_minimum_msat,
            inbound_htlc_maximum_msat,

            balance_msat,
            outbound_capacity_msat,
            next_outbound_htlc_limit_msat,

            inbound_capacity_msat,

            force_close_spend_delay,
            is_public,
            is_outbound,
            is_channel_ready,
            is_usable,
            config,
            ..
        }: ChannelDetails,
    ) -> Self {
        let channel_id = ChannelId(channel_id);
        let funding_txo = funding_txo.map(LxOutPoint::from);
        let counterparty_node_id = NodePk(counterparty.node_id);
        let channel_value = u32::try_from(channel_value_satoshis)
            .expect("We should not have a 42 BTC+ channel")
            .apply(Amount::from_sats_u32);
        let punishment_reserve = unspendable_punishment_reserve
            .unwrap_or(0)
            .apply(u32::try_from)
            .expect("Reserve was greater than 42 BTC")
            .apply(Amount::from_sats_u32);
        let is_ready = is_channel_ready;

        let balance = Amount::from_msat(balance_msat);
        let outbound_capacity = Amount::from_msat(outbound_capacity_msat);
        let next_outbound_htlc_limit =
            Amount::from_msat(next_outbound_htlc_limit_msat);

        let their_balance = channel_value
            .checked_sub(balance)
            .expect("Our balance was higher than the total channel value");
        let inbound_capacity = Amount::from_msat(inbound_capacity_msat);

        let config = config.expect("Only None prior to LDK 0.0.109");
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
            .expect("Only None prior to LDK 0.0.107")
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

        Self {
            channel_id,
            funding_txo,
            counterparty_node_id,
            channel_value,
            punishment_reserve,
            force_close_spend_delay,
            is_public,
            is_outbound,
            is_ready,
            is_usable,
            balance,
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
        }
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
