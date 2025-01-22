//! Type aliases which prevent most LDK generics from infecting Lexe APIs.

use std::sync::{Arc, Mutex};

use lightning::{
    chain::{chainmonitor::ChainMonitor, channelmonitor::ChannelMonitor},
    ln::{
        channelmanager::ChannelManager,
        peer_handler::{IgnoringMessageHandler, PeerManager},
    },
    onion_message::messenger::{DefaultMessageRouter, OnionMessenger},
    routing::{
        gossip::{NetworkGraph, P2PGossipSync},
        router::DefaultRouter,
        scoring::{ProbabilisticScorer, ProbabilisticScoringFeeParameters},
        utxo::UtxoLookup,
    },
    sign::InMemorySigner,
};
use lightning_transaction_sync::EsploraSyncClient;

use crate::{
    esplora::LexeEsplora, keys_manager::LexeKeysManager,
    logger::LexeTracingLogger, p2p::ConnectionTx,
};

// --- Partial aliases --- //
// - All are prefixed with "Lexe" and contain at least one more generic.
// - The last generic is filled by an alias in the consuming crate.
// - Lexicographically sorted.

pub type LexeChainMonitorType<PERSISTER> = ChainMonitor<
    SignerType,
    Arc<EsploraSyncClientType>,
    Arc<BroadcasterType>,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
    PERSISTER,
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

pub type LexeOnionMessengerType<CHANNEL_MANAGER> = OnionMessenger<
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    LexeTracingLogger,
    CHANNEL_MANAGER,
    Arc<MessageRouterType>,
    // OffersMessageHandler
    // TODO(max): Need a OffersMessageHandler for BOLT 12
    IgnoringMessageHandler,
    // AsyncPaymentsMessageHandler
    IgnoringMessageHandler,
    // CustomOnionMessageHandler
    IgnoringMessageHandler,
>;

pub type LexePeerManagerType<CHANNEL_MANAGER> = PeerManager<
    ConnectionTx,
    CHANNEL_MANAGER,
    Arc<P2PGossipSyncType>,
    Arc<LexeOnionMessengerType<CHANNEL_MANAGER>>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
    Arc<LexeKeysManager>,
>;

// --- Full type aliases --- //
// - Fully concrete.
// - Lexicographically sorted.

pub type BroadcasterType = LexeEsplora;

pub type ChannelMonitorType = ChannelMonitor<SignerType>;

pub type EsploraSyncClientType = EsploraSyncClient<LexeTracingLogger>;

pub type FeeEstimatorType = LexeEsplora;

pub type MessageRouterType = DefaultMessageRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    Arc<LexeKeysManager>,
>;

pub type NetworkGraphType = NetworkGraph<LexeTracingLogger>;

pub type P2PGossipSyncType = P2PGossipSync<
    Arc<NetworkGraphType>,
    Arc<UtxoLookupType>,
    LexeTracingLogger,
>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type RouterType = DefaultRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    Arc<LexeKeysManager>,
    Arc<Mutex<ProbabilisticScorerType>>,
    ProbabilisticScoringFeeParameters,
    ProbabilisticScorerType,
>;

pub type SignerType = InMemorySigner;

// TODO(max): Revisit - why are we using dynamic dispatch here?
pub type UtxoLookupType = dyn UtxoLookup + Send + Sync;
