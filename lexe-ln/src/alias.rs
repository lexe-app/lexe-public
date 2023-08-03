use std::sync::{Arc, Mutex};

use lightning::{
    chain::{chainmonitor::ChainMonitor, channelmonitor::ChannelMonitor},
    ln::{
        channelmanager::ChannelManager,
        peer_handler::{IgnoringMessageHandler, PeerManager},
    },
    onion_message::{DefaultMessageRouter, OnionMessenger},
    routing::{
        gossip::{NetworkGraph, P2PGossipSync},
        router::DefaultRouter,
        scoring::{ProbabilisticScorer, ProbabilisticScoringFeeParameters},
        utxo::UtxoLookup,
    },
    sign::InMemorySigner,
};
use lightning_net_tokio::SocketDescriptor;
use lightning_transaction_sync::EsploraSyncClient;

use crate::{
    esplora::LexeEsplora, keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
};

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

pub type UtxoLookupType = dyn UtxoLookup + Send + Sync;

pub type P2PGossipSyncType = P2PGossipSync<
    Arc<NetworkGraphType>,
    Arc<UtxoLookupType>,
    LexeTracingLogger,
>;

pub type LexeChannelManagerType<PERSISTER> = ChannelManager<
    Arc<LexeChainMonitorType<PERSISTER>>,
    Arc<BroadcasterType>,
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    Arc<FeeEstimatorType>,
    Arc<RouterType>,
    LexeTracingLogger,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type OnionMessengerType = OnionMessenger<
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    LexeTracingLogger,
    Arc<DefaultMessageRouter>,
    IgnoringMessageHandler,
    IgnoringMessageHandler,
>;

pub type LexePeerManagerType<CHANNEL_MANAGER> = PeerManager<
    SocketDescriptor,
    CHANNEL_MANAGER,
    Arc<P2PGossipSyncType>,
    Arc<OnionMessengerType>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
    Arc<LexeKeysManager>,
>;

pub type RouterType = DefaultRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    Arc<Mutex<ProbabilisticScorerType>>,
    ProbabilisticScoringFeeParameters,
    ProbabilisticScorerType,
>;
