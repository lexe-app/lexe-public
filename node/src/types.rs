//! Core types and data structures used throughout the lexe-node.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{ensure, format_err};
use common::hex;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::{self, chainmonitor, Access, Filter};
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::ln::peer_handler::SimpleArcPeerManager;
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_background_processor::GossipSync;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;
use subtle::ConstantTimeEq;

use crate::bitcoind_client::BitcoindClient;
use crate::logger::LdkTracingLogger;
use crate::persister::LexePersister;

pub type UserId = i64;
pub type Port = u16;
pub type InstanceId = String;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type ChainMonitorType = chainmonitor::ChainMonitor<
    InMemorySigner,
    Arc<dyn Filter + Send + Sync>,
    Arc<BitcoindClient>,
    Arc<BitcoindClient>,
    Arc<LdkTracingLogger>,
    Arc<LexePersister>,
>;

pub type PeerManagerType = SimpleArcPeerManager<
    SocketDescriptor,
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    dyn chain::Access + Send + Sync,
    LdkTracingLogger,
>;

pub type ChannelManagerType = SimpleArcChannelManager<
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    LdkTracingLogger,
>;

pub type ChannelMonitorType = ChannelMonitor<InMemorySigner>;

/// We use this strange tuple because LDK impl'd `Listen` for it
pub type ChannelMonitorListenerType = (
    ChannelMonitorType,
    Arc<BitcoindClient>,
    Arc<BitcoindClient>,
    Arc<LdkTracingLogger>,
);

pub type InvoicePayerType<E> = payment::InvoicePayer<
    Arc<ChannelManagerType>,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    Arc<LdkTracingLogger>,
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

pub type BroadcasterType = BitcoindClient;
pub type FeeEstimatorType = BitcoindClient;

pub type LoggerType = Arc<LdkTracingLogger>;

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: MillisatAmount,
}

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

/// The information required to connect to a bitcoind instance via RPC
#[derive(Debug, PartialEq, Eq)]
pub struct BitcoindRpcInfo {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: Port,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeAlias([u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Network(bitcoin::Network);

#[derive(Clone)]
pub struct AuthToken([u8; Self::LENGTH]);

// -- impl BitcoindRpcInfo -- //

impl BitcoindRpcInfo {
    fn parse_str(s: &str) -> Option<Self> {
        // format: <username>:<password>@<host>:<port>

        let mut parts = s.split(':');
        let (username, pass_host, port) =
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(username), Some(pass_host), Some(port), None) => {
                    (username, pass_host, port)
                }
                _ => return None,
            };

        let mut parts = pass_host.split('@');
        let (password, host) = match (parts.next(), parts.next(), parts.next())
        {
            (Some(password), Some(host), None) => (password, host),
            _ => return None,
        };

        let port = Port::from_str(port).ok()?;

        Some(Self {
            username: username.to_string(),
            password: password.to_string(),
            host: host.to_string(),
            port,
        })
    }
}

impl FromStr for BitcoindRpcInfo {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s)
            .ok_or_else(|| format_err!("Invalid bitcoind rpc URL"))
    }
}

// -- impl NodeAlias -- //

impl NodeAlias {
    pub fn new(inner: [u8; 32]) -> Self {
        Self(inner)
    }
}

impl FromStr for NodeAlias {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        ensure!(
            bytes.len() <= 32,
            "node alias can't be longer than 32 bytes"
        );

        let mut alias = [0_u8; 32];
        alias[..bytes.len()].copy_from_slice(bytes);

        Ok(Self(alias))
    }
}

impl fmt::Display for NodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for b in self.0.iter() {
            let c = *b as char;
            if c == '\0' {
                break;
            }
            if c.is_ascii_graphic() || c == ' ' {
                continue;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for NodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// -- impl Network -- //

impl Network {
    pub fn into_inner(self) -> bitcoin::Network {
        self.0
    }
}

impl Default for Network {
    fn default() -> Self {
        Self(bitcoin::Network::Testnet)
    }
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let network = bitcoin::Network::from_str(s)?;
        ensure!(
            !matches!(network, bitcoin::Network::Bitcoin),
            "Mainnet is disabled for now"
        );
        Ok(Self(network))
    }
}

// -- impl AuthToken -- //

impl AuthToken {
    const LENGTH: usize = 32;

    pub fn new(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    #[cfg(test)]
    pub fn string(&self) -> String {
        hex::encode(self.0.as_slice())
    }
}

// AuthToken is a secret. We need to compare in constant time.

impl ConstantTimeEq for AuthToken {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.as_slice().ct_eq(other.0.as_slice())
    }
}

impl PartialEq for AuthToken {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).into()
    }
}

impl Eq for AuthToken {}

impl FromStr for AuthToken {
    type Err = hex::DecodeError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; Self::LENGTH];
        hex::decode_to_slice_ct(hex, bytes.as_mut_slice())
            .map(|()| Self::new(bytes))
    }
}

impl fmt::Debug for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        f.write_str("AuthToken(..)")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_bitcoind_rpc_info() {
        let expected = BitcoindRpcInfo {
            username: "hello".to_string(),
            password: "world".to_string(),
            host: "foo.bar".to_string(),
            port: 1234,
        };
        let actual =
            BitcoindRpcInfo::from_str("hello:world@foo.bar:1234").unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_parse_node_alias() {
        let expected = NodeAlias(*b"hello, world - this is lexe\0\0\0\0\0");
        let actual =
            NodeAlias::from_str("hello, world - this is lexe").unwrap();
        assert_eq!(expected, actual);
    }
}
