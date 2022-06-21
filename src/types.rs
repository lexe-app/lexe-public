use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use lightning::chain;
use lightning::chain::chainmonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::Filter;
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::SimpleArcPeerManager;
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;

use crate::bitcoind_client::BitcoindClient;
use crate::logger::StdOutLogger;
use crate::persister::PostgresPersister;

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

pub type GossipSyncType<P, G, A, L> =
    lightning_background_processor::GossipSync<
        P,
        Arc<RapidGossipSync<G, L>>,
        G,
        A,
        L,
    >;

pub type NetworkGraphType = NetworkGraph<LoggerType>;

pub type LoggerType = Arc<StdOutLogger>;

pub enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

pub struct MillisatAmount(pub Option<u64>);

impl fmt::Display for MillisatAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(amt) => write!(f, "{}", amt),
            None => write!(f, "unknown"),
        }
    }
}

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: MillisatAmount,
}

pub struct NodeAlias<'a>(pub &'a [u8; 32]);

impl fmt::Display for NodeAlias<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let alias = self
            .0
            .iter()
            .map(|b| *b as char)
            .take_while(|c| *c != '\0')
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect::<String>();
        write!(f, "{}", alias)
    }
}
