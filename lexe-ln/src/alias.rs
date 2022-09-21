use std::sync::Arc;

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::Filter;

use crate::bitcoind::LexeBitcoind;
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
