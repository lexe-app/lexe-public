use crate::types::{
    ChainMonitorType, ChannelManagerType, GossipSyncType, HTLCStatus,
    InvoicePayerType, LoggerType, MillisatAmount, NetworkGraphType, NodeAlias,
    PaymentInfo, PaymentInfoStorageType, PeerManagerType, Port,
    ProbabilisticScorerType, UserId,
};

mod api;
mod bitcoind_client;
mod cli;
mod convert;
mod event_handler;
mod hex_utils;
mod init;
mod logger;
mod persister;
mod types;

#[tokio::main]
pub async fn main() {
    match init::start_ldk().await {
        Ok(()) => {}
        Err(e) => println!("Error: {:#}", e),
    }
}
