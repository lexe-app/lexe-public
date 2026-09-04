//! Concrete aliases for generic types and partial aliases from [`lexe_ln`].

use std::sync::Arc;

use lexe_ln::alias::{
    LexeChainMonitorType, LexeChannelManagerType, LexeOnionMessengerType,
    LexePeerManagerType,
};
use lightning::ln::peer_handler::IgnoringMessageHandler;

use crate::persister::RecoveryPersister;

pub(crate) type ChainMonitorType = LexeChainMonitorType<Arc<RecoveryPersister>>;

pub(crate) type ChannelManagerType =
    LexeChannelManagerType<Arc<RecoveryPersister>>;

pub(crate) type OnionMessengerType =
    LexeOnionMessengerType<Arc<ChannelManagerType>>;

pub(crate) type PeerManagerType = LexePeerManagerType<
    Arc<ChannelManagerType>,
    // RoutingMessageHandler: The recovery node never routes, so it has no use
    // for gossip.
    IgnoringMessageHandler,
    Arc<RecoveryPersister>,
>;
