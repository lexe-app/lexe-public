use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lightning::chain::chainmonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::Filter;
use lightning::chain::{self, Access};
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::SimpleArcPeerManager;
use lightning::ln::PaymentHash;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_background_processor::GossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;

use crate::bitcoind_client::BitcoindClient;
use crate::logger::StdOutLogger;
use crate::persister::PostgresPersister;
use crate::structs::PaymentInfo;

pub type UserId = i64;
pub type Port = u16;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type ChainMonitorType = chainmonitor::ChainMonitor<
    InMemorySigner,
    Arc<dyn Filter + Send + Sync>,
    Arc<BitcoindClient>,
    Arc<BitcoindClient>,
    Arc<StdOutLogger>,
    Arc<PostgresPersister>,
>;

pub type PeerManagerType = SimpleArcPeerManager<
    SocketDescriptor,
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    dyn chain::Access + Send + Sync,
    StdOutLogger,
>;

pub type ChannelManagerType = SimpleArcChannelManager<
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    StdOutLogger,
>;

pub type ChannelMonitorType = ChannelMonitor<InMemorySigner>;

/// We use this strange tuple because LDK impl'd `Listen` for it
pub type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BitcoindClient>,
    Arc<BitcoindClient>,
    Arc<StdOutLogger>,
);

pub type InvoicePayerType<E> = payment::InvoicePayer<
    Arc<ChannelManagerType>,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    Arc<StdOutLogger>,
    E,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LoggerType>;

pub type RouterType = DefaultRouter<Arc<NetworkGraphType>, LoggerType>;

pub type GossipSyncType = GossipSync<
    Arc<
        P2PGossipSync<
            Arc<NetworkGraphType>,
            Arc<dyn Access + Send + Sync>,
            LoggerType,
        >,
    >,
    Arc<RapidGossipSync<Arc<NetworkGraphType>, LoggerType>>,
    Arc<NetworkGraphType>,
    Arc<dyn Access + Send + Sync>,
    LoggerType,
>;

pub type P2PGossipSyncType = P2PGossipSync<
    Arc<NetworkGraphType>,
    Arc<dyn Access + Send + Sync>,
    LoggerType,
>;

pub type NetworkGraphType = NetworkGraph<LoggerType>;

pub type LoggerType = Arc<StdOutLogger>;
