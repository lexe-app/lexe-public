use lexe_ln::alias::{
    LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
};

use crate::channel_manager::NodeChannelManager;
use crate::persister::NodePersister;

pub(crate) type ChannelManagerType = LexeChannelManagerType<NodePersister>;

pub(crate) type ChainMonitorType = LexeChainMonitorType<NodePersister>;

pub(crate) type PeerManagerType = LexePeerManagerType<NodeChannelManager>;
