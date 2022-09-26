use std::sync::Arc;

use lexe_ln::alias::{
    BroadcasterType, ChannelMonitorType, FeeEstimatorType,
    LexeChainMonitorType, LexeChannelManagerType, LexeInvoicePayerType,
    LexePeerManagerType,
};
use lexe_ln::logger::LexeTracingLogger;

use crate::channel_manager::NodeChannelManager;
use crate::event_handler::NodeEventHandler;
use crate::persister::NodePersister;

pub(crate) type ChannelManagerType = LexeChannelManagerType<NodePersister>;

pub(crate) type ChainMonitorType = LexeChainMonitorType<NodePersister>;

pub(crate) type PeerManagerType = LexePeerManagerType<NodeChannelManager>;

/// This is the tuple that LDK impl'd `Listen` for
pub(crate) type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
);

pub(crate) type InvoicePayerType =
    LexeInvoicePayerType<NodeChannelManager, NodeEventHandler>;
