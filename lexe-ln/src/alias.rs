use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::Access;
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::ln::PaymentHash;
use lightning::onion_message::OnionMessenger;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::router::DefaultRouter;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_invoice::payment::InvoicePayer;
use lightning_net_tokio::SocketDescriptor;
use lightning_transaction_sync::EsploraSyncClient;

use crate::esplora::LexeEsplora;
use crate::invoice::PaymentInfo;
use crate::keys_manager::LexeKeysManager;
use crate::logger::LexeTracingLogger;

pub type SignerType = InMemorySigner;

pub type ChannelMonitorType = ChannelMonitor<SignerType>;

pub type BroadcasterType = LexeEsplora;
pub type FeeEstimatorType = LexeEsplora;

pub type EsploraSyncClientType = EsploraSyncClient<LexeTracingLogger>;

pub type LexeChainMonitorType<PERSISTER> = ChainMonitor<
    SignerType,
    Arc<EsploraSyncClientType>,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
    PERSISTER,
>;

pub type NetworkGraphType = NetworkGraph<LexeTracingLogger>;

pub type ChainAccessType = dyn Access + Send + Sync;

pub type P2PGossipSyncType = P2PGossipSync<
    Arc<NetworkGraphType>,
    Arc<ChainAccessType>,
    LexeTracingLogger,
>;

pub type LexeChannelManagerType<PERSISTER> = ChannelManager<
    Arc<LexeChainMonitorType<PERSISTER>>,
    Arc<BroadcasterType>,
    LexeKeysManager,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type OnionMessengerType =
    OnionMessenger<LexeKeysManager, LexeTracingLogger, IgnoringMessageHandler>;

pub type LexePeerManagerType<CHANNEL_MANAGER> = PeerManager<
    SocketDescriptor,
    CHANNEL_MANAGER,
    Arc<P2PGossipSyncType>,
    Arc<OnionMessengerType>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
>;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type RouterType = DefaultRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    Arc<Mutex<ProbabilisticScorerType>>,
>;

// TODO(max): Expand this further to InvoicePayerUsingTime?
pub type LexeInvoicePayerType<CHANNEL_MANAGER, EVENT_HANDLER> =
    InvoicePayer<CHANNEL_MANAGER, RouterType, LexeTracingLogger, EVENT_HANDLER>;
