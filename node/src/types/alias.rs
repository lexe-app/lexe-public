use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lexe_ln::alias::{
    BroadcasterType, ChannelMonitorType, FeeEstimatorType,
    LexeChainMonitorType, LexeChannelManagerType, NetworkGraphType,
    P2PGossipSyncType, ProbabilisticScorerType, SignerType,
};
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::ln::PaymentHash;
use lightning::onion_message::OnionMessenger;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;

use crate::event_handler::LdkEventHandler;
use crate::lexe::channel_manager::NodeChannelManager;
use crate::lexe::persister::NodePersister;
use crate::types::PaymentInfo;

pub(crate) type ChannelManagerType = LexeChannelManagerType<NodePersister>;

pub(crate) type PaymentInfoStorageType =
    Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub(crate) type ChainMonitorType = LexeChainMonitorType<NodePersister>;

pub(crate) type OnionMessengerType =
    OnionMessenger<SignerType, LexeKeysManager, LexeTracingLogger>;

pub(crate) type PeerManagerType = PeerManager<
    SocketDescriptor,
    NodeChannelManager,
    Arc<P2PGossipSyncType>,
    Arc<OnionMessengerType>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
>;

/// This is the tuple that LDK impl'd `Listen` for
pub(crate) type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
);

pub(crate) type InvoicePayerType = payment::InvoicePayer<
    NodeChannelManager,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    LexeTracingLogger,
    LdkEventHandler,
>;

pub(crate) type RouterType =
    DefaultRouter<Arc<NetworkGraphType>, LexeTracingLogger>;
