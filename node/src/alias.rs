use lexe_ln::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
    },
    payments::manager::PaymentsManager,
};

use crate::{channel_manager::NodeChannelManager, persister::NodePersister};

pub(crate) type ChannelManagerType = LexeChannelManagerType<NodePersister>;

pub type ChainMonitorType = LexeChainMonitorType<NodePersister>;

pub(crate) type PeerManagerType = LexePeerManagerType<NodeChannelManager>;

pub type NodePaymentsManagerType =
    PaymentsManager<NodeChannelManager, NodePersister>;
