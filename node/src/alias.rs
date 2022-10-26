use std::sync::Arc;

use lexe_ln::alias::{
    LexeChainMonitorType, LexeChannelManagerType, LexeInvoicePayerType,
    LexePeerManagerType,
};

use crate::api::ApiClient;
use crate::channel_manager::NodeChannelManager;
use crate::event_handler::NodeEventHandler;
use crate::persister::NodePersister;

pub type ApiClientType = Arc<dyn ApiClient + Send + Sync>;

pub(crate) type ChannelManagerType = LexeChannelManagerType<NodePersister>;

pub(crate) type ChainMonitorType = LexeChainMonitorType<NodePersister>;

pub(crate) type PeerManagerType = LexePeerManagerType<NodeChannelManager>;

pub type InvoicePayerType =
    LexeInvoicePayerType<NodeChannelManager, NodeEventHandler>;
