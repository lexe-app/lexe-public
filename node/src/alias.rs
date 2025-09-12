//! Concrete aliases for generic types and partial aliases from [`lexe_ln`].

use std::sync::Arc;

use lexe_ln::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexeOnionMessengerType,
        LexePeerManagerType, P2PGossipSyncType,
    },
    payments::manager::PaymentsManager,
};

use crate::{channel_manager::NodeChannelManager, persister::NodePersister};

pub(crate) type ChainMonitorType = LexeChainMonitorType<Arc<NodePersister>>;

pub(crate) type ChannelManagerType = LexeChannelManagerType<Arc<NodePersister>>;

pub(crate) type OnionMessengerType = LexeOnionMessengerType<NodeChannelManager>;

pub(crate) type PaymentsManagerType =
    PaymentsManager<NodeChannelManager, Arc<NodePersister>>;

pub(crate) type PeerManagerType =
    LexePeerManagerType<NodeChannelManager, Arc<P2PGossipSyncType>>;
