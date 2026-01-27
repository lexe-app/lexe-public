//! Type aliases which prevent most LDK generics from infecting Lexe APIs.

use std::sync::Arc;

use lightning::{
    chain::{chainmonitor::ChainMonitor, channelmonitor::ChannelMonitor},
    ln::{
        channelmanager::ChannelManager,
        peer_handler::{IgnoringMessageHandler, PeerManager},
    },
    onion_message::messenger::OnionMessenger,
    routing::{
        gossip::NetworkGraph, scoring::ProbabilisticScorer, utxo::UtxoLookup,
    },
    sign::InMemorySigner,
};
use lightning_transaction_sync::EsploraSyncClient;

use crate::{
    esplora::FeeEstimates, keys_manager::LexeKeysManager,
    logger::LexeTracingLogger, message_router::LexeMessageRouter,
    p2p::ConnectionTx, route::LexeRouter, tx_broadcaster::TxBroadcaster,
};

// --- Partial aliases --- //
// - All are prefixed with "Lexe" and contain at least one more generic.
// - The last generic is filled by an alias in the consuming crate.
// - Lexicographically sorted.

pub type LexeChainMonitorType<PERSISTER> = ChainMonitor<
    SignerType,
    Arc<EsploraSyncClientType>,
    BroadcasterType,
    Arc<FeeEstimatorType>,
    LexeTracingLogger,
    PERSISTER,
>;

pub type LexeChannelManagerType<PERSISTER> = ChannelManager<
    Arc<LexeChainMonitorType<PERSISTER>>,
    BroadcasterType,
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    Arc<FeeEstimatorType>,
    Arc<RouterType>,
    Arc<MessageRouterType>,
    LexeTracingLogger,
>;

pub type LexeOnionMessengerType<CHANNEL_MANAGER> = OnionMessenger<
    Arc<LexeKeysManager>,
    Arc<LexeKeysManager>,
    LexeTracingLogger,
    CHANNEL_MANAGER,
    Arc<MessageRouterType>,
    // OffersMessageHandler
    CHANNEL_MANAGER,
    // AsyncPaymentsMessageHandler
    IgnoringMessageHandler,
    // DNSResolverMessageHandler
    // TODO(phlip9): impl for BIP 353?
    IgnoringMessageHandler,
    // CustomOnionMessageHandler
    IgnoringMessageHandler,
>;

pub type LexePeerManagerType<CHANNEL_MANAGER, RMH> = PeerManager<
    ConnectionTx,
    CHANNEL_MANAGER,
    // RoutingMessageHandler
    RMH,
    Arc<LexeOnionMessengerType<CHANNEL_MANAGER>>,
    LexeTracingLogger,
    Arc<IgnoringMessageHandler>,
    Arc<LexeKeysManager>,
>;

// --- Full type aliases --- //
// - Fully concrete.
// - Lexicographically sorted.

pub type BroadcasterType = TxBroadcaster;

pub type ChannelMonitorType = ChannelMonitor<SignerType>;

pub type EsploraSyncClientType = EsploraSyncClient<LexeTracingLogger>;

pub type FeeEstimatorType = FeeEstimates;

pub type MessageRouterType = LexeMessageRouter;

pub type NetworkGraphType = NetworkGraph<LexeTracingLogger>;

pub type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LexeTracingLogger>;

pub type RouterType = LexeRouter;

pub type SignerType = InMemorySigner;

// TODO(max): Revisit - why are we using dynamic dispatch here?
pub type UtxoLookupType = dyn UtxoLookup + Send + Sync;
