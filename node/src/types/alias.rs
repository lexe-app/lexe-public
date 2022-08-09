use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::{Access, Filter};
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::ln::PaymentHash;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;

use crate::event_handler::LdkEventHandler;
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::persister::LexePersister;
use crate::types::PaymentInfo;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type SignerType = InMemorySigner;

pub type ChainMonitorType = ChainMonitor<
    SignerType,
    Arc<dyn Filter + Send + Sync>,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
    LexePersister,
>;

pub type PeerManagerType = PeerManager<
    SocketDescriptor,
    LexeChannelManager,
    Arc<P2PGossipSyncType>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
>;

pub type ChannelManagerType = ChannelManager<
    SignerType,
    Arc<ChainMonitorType>,
    Arc<BroadcasterType>,
    LexeKeysManager,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
>;

pub type ChannelMonitorType = ChannelMonitor<SignerType>;

/// This is the tuple that LDK impl'd `Listen` for
pub type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
);

pub type InvoicePayerType = payment::InvoicePayer<
    LexeChannelManager,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    LexeTracingLogger,
    LdkEventHandler,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LoggerType>;

pub type RouterType = DefaultRouter<Arc<NetworkGraphType>, LoggerType>;

pub type P2PGossipSyncType =
    P2PGossipSync<Arc<NetworkGraphType>, Arc<ChainAccessType>, LoggerType>;

pub type NetworkGraphType = NetworkGraph<LoggerType>;

pub type ChainAccessType = dyn Access + Send + Sync;

pub type WalletType = LexeBitcoind;
pub type BlockSourceType = LexeBitcoind;
pub type BroadcasterType = LexeBitcoind;
pub type FeeEstimatorType = LexeBitcoind;

pub type LoggerType = LexeTracingLogger;
