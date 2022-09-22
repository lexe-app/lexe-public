use std::sync::Arc;

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::{Access, Filter};
use lightning::ln::channelmanager::ChannelManager;
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;

use crate::bitcoind::LexeBitcoind;
use crate::keys_manager::LexeKeysManager;
use crate::logger::LexeTracingLogger;

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
