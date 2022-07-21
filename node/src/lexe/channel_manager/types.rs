use lightning::ln::channelmanager::{ChannelCounterparty, ChannelDetails};
use serde::Serialize;

use crate::types::{LxOutPoint, LxPublicKey};

#[derive(Serialize)]
pub struct LxChannelDetails {
    pub channel_id: [u8; 32],
    pub counterparty: LxChannelCounterparty,
    pub funding_txo: Option<LxOutPoint>,
    // pub channel_type: Option<ChannelTypeFeatures>, // Sealed
    pub short_channel_id: Option<u64>,
    pub outbound_scid_alias: Option<u64>,
    pub inbound_scid_alias: Option<u64>,
    pub channel_value_satoshis: u64,
    pub unspendable_punishment_reserve: Option<u64>,
    pub user_channel_id: u64,
    pub balance_msat: u64,
    pub outbound_capacity_msat: u64,
    pub next_outbound_htlc_limit_msat: u64,
    pub inbound_capacity_msat: u64,
    pub confirmations_required: Option<u32>,
    pub force_close_spend_delay: Option<u16>,
    pub is_outbound: bool,
    pub is_channel_ready: bool,
    pub is_usable: bool,
    pub is_public: bool,
    pub inbound_htlc_minimum_msat: Option<u64>,
    pub inbound_htlc_maximum_msat: Option<u64>,
}

impl From<ChannelDetails> for LxChannelDetails {
    fn from(cd: ChannelDetails) -> Self {
        Self {
            channel_id: cd.channel_id,
            counterparty: LxChannelCounterparty::from(cd.counterparty),
            funding_txo: cd.funding_txo.map(LxOutPoint::from),
            short_channel_id: cd.short_channel_id,
            outbound_scid_alias: cd.outbound_scid_alias,
            inbound_scid_alias: cd.inbound_scid_alias,
            channel_value_satoshis: cd.channel_value_satoshis,
            unspendable_punishment_reserve: cd.unspendable_punishment_reserve,
            user_channel_id: cd.user_channel_id,
            balance_msat: cd.balance_msat,
            outbound_capacity_msat: cd.outbound_capacity_msat,
            next_outbound_htlc_limit_msat: cd.next_outbound_htlc_limit_msat,
            inbound_capacity_msat: cd.inbound_capacity_msat,
            confirmations_required: cd.confirmations_required,
            force_close_spend_delay: cd.force_close_spend_delay,
            is_outbound: cd.is_outbound,
            is_channel_ready: cd.is_channel_ready,
            is_usable: cd.is_usable,
            is_public: cd.is_public,
            inbound_htlc_minimum_msat: cd.inbound_htlc_minimum_msat,
            inbound_htlc_maximum_msat: cd.inbound_htlc_maximum_msat,
        }
    }
}

#[derive(Serialize)]
pub struct LxChannelCounterparty {
    pub node_id: LxPublicKey,
    // pub features: InitFeatures,                              // Sealed
    pub unspendable_punishment_reserve: u64,
    // pub forwarding_info: Option<CounterpartyForwardingInfo>, // Not needed
    pub outbound_htlc_minimum_msat: Option<u64>,
    pub outbound_htlc_maximum_msat: Option<u64>,
}

impl From<ChannelCounterparty> for LxChannelCounterparty {
    fn from(ccp: ChannelCounterparty) -> Self {
        Self {
            node_id: LxPublicKey::from(ccp.node_id), // CCP's node id lol
            unspendable_punishment_reserve: ccp.unspendable_punishment_reserve,
            outbound_htlc_minimum_msat: ccp.outbound_htlc_minimum_msat,
            outbound_htlc_maximum_msat: ccp.outbound_htlc_maximum_msat,
        }
    }
}
