use std::sync::Arc;

use lexe_ln::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
    },
    payments::manager::PaymentsManager,
};

use crate::{channel_manager::NodeChannelManager, persister::NodePersister};

pub(crate) type ChannelManagerType = LexeChannelManagerType<Arc<NodePersister>>;

pub type ChainMonitorType = LexeChainMonitorType<Arc<NodePersister>>;

pub(crate) type PeerManagerType = LexePeerManagerType<NodeChannelManager>;

pub type NodePaymentsManagerType =
    PaymentsManager<NodeChannelManager, Arc<NodePersister>>;
