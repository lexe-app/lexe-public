use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::{Access, Filter};
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::ln::PaymentHash;
use lightning::onion_message::OnionMessenger;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_invoice::payment::InvoicePayer;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;

use crate::bitcoind::LexeBitcoind;
use crate::keys_manager::LexeKeysManager;
use crate::logger::LexeTracingLogger;
use crate::types::PaymentInfo;

pub type SignerType = InMemorySigner;

pub type ChannelMonitorType = ChannelMonitor<SignerType>;

pub type WalletType = LexeBitcoind;
pub type BlockSourceType = LexeBitcoind;
pub type BroadcasterType = LexeBitcoind;
pub type FeeEstimatorType = LexeBitcoind;

pub type LexeChainMonitorType<PERSISTER> = ChainMonitor<
    SignerType,
    Arc<dyn Filter + Send + Sync>,
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
    SignerType,
    Arc<LexeChainMonitorType<PERSISTER>>,
    Arc<BroadcasterType>,
    LexeKeysManager,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type OnionMessengerType =
    OnionMessenger<SignerType, LexeKeysManager, LexeTracingLogger>;

pub type LexePeerManagerType<CHANNELMANAGER> = PeerManager<
    SocketDescriptor,
    CHANNELMANAGER,
    Arc<P2PGossipSyncType>,
    Arc<OnionMessengerType>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
>;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type RouterType = DefaultRouter<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type LexeInvoicePayerType<CHANNELMANAGER, EVENTHANDLER> = InvoicePayer<
    CHANNELMANAGER,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    LexeTracingLogger,
    EVENTHANDLER,
>;

/// This is the tuple that LDK impl'd `Listen` for
pub type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
);
