//! Concrete aliases for generic types and partial aliases from [`lexe_ln`].

use std::sync::Arc;

use lexe_ln::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexeOnionMessengerType,
        LexePeerManagerType,
    },
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    tx_broadcaster::TxBroadcaster,
};
use lightning::{
    events::bump_transaction::BumpTransactionEventHandler,
    ln::peer_handler::IgnoringMessageHandler,
};

use crate::{
    coin_selector::RecoveryCoinSelector, persister::RecoveryPersister,
};

pub(crate) type BumpTransactionEventHandlerType = BumpTransactionEventHandler<
    TxBroadcaster,
    RecoveryCoinSelector,
    Arc<LexeKeysManager>,
    LexeTracingLogger,
>;

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
