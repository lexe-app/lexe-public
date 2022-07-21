//! Core types and data structures used throughout the lexe-node.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::ensure;
use bitcoin::hash_types::Txid;
use bitcoin::hashes::sha256d;
use bitcoin::secp256k1::PublicKey;
use common::hex;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::channelmonitor::ChannelMonitor;
use lightning::chain::keysinterface::InMemorySigner;
use lightning::chain::transaction::OutPoint;
use lightning::chain::{Access, Filter};
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager};
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning_background_processor::GossipSync;
use lightning_invoice::utils::DefaultRouter;
use lightning_invoice::{payment, Currency};
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use subtle::ConstantTimeEq;

use crate::api::ApiClient;
use crate::event_handler::LdkEventHandler;
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::LexeChannelManager;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::persister::LexePersister;

pub type UserId = i64;
pub type Port = u16;
pub type InstanceId = String;
pub type EnclaveId = String;
pub type Measurement = String;

pub type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

pub type ApiClientType = Arc<dyn ApiClient + Send + Sync>;

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
    Arc<
        P2PGossipSync<
            Arc<NetworkGraph<LexeTracingLogger>>,
            Arc<ChainAccessType>,
            LexeTracingLogger,
        >,
    >,
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

/// We use this strange tuple because LDK impl'd `Listen` for it
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

pub type GossipSyncType = GossipSync<
    Arc<P2PGossipSync<Arc<NetworkGraphType>, Arc<ChainAccessType>, LoggerType>>,
    Arc<RapidGossipSync<Arc<NetworkGraphType>, LoggerType>>,
    Arc<NetworkGraphType>,
    Arc<ChainAccessType>,
    LoggerType,
>;

pub type P2PGossipSyncType =
    P2PGossipSync<Arc<NetworkGraphType>, Arc<ChainAccessType>, LoggerType>;

pub type NetworkGraphType = NetworkGraph<LoggerType>;

pub type ChainAccessType = dyn Access + Send + Sync;

pub type BlockSourceType = LexeBitcoind;
pub type BroadcasterType = LexeBitcoind;
pub type FeeEstimatorType = LexeBitcoind;

pub type LoggerType = LexeTracingLogger;

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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeAlias([u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Network(bitcoin::Network);

#[derive(Clone)]
pub struct AuthToken([u8; Self::LENGTH]);

#[derive(Serialize)]
pub struct LxOutPoint {
    pub txid: LxTxid,
    pub index: u16,
}

impl From<OutPoint> for LxOutPoint {
    fn from(op: OutPoint) -> Self {
        Self {
            txid: LxTxid::from(op.txid),
            index: op.index,
        }
    }
}

/// Wraps bitcoin::hash_types::Txid to implement Serialize.
pub struct LxTxid(Txid);

impl From<Txid> for LxTxid {
    fn from(txid: Txid) -> Self {
        Self::from_hash(txid.as_hash())
    }
}

impl LxTxid {
    pub fn from_hash(hash: sha256d::Hash) -> Self {
        Self(Txid::from_hash(hash))
    }

    pub fn as_hash(&self) -> sha256d::Hash {
        self.0.as_hash()
    }
}

impl Serialize for LxTxid {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let hex_str = hex::encode(self.as_hash().deref());
        serializer.serialize_str(&hex_str)
    }
}

/// Wraps PublicKey to implement Serialize and Deserialize.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct LxPublicKey(PublicKey);

impl From<PublicKey> for LxPublicKey {
    fn from(pk: PublicKey) -> Self {
        Self(pk)
    }
}

impl FromStr for LxPublicKey {
    type Err = bitcoin::secp256k1::Error;
    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        // Deserialize using PublicKey's from_str impl
        let inner = PublicKey::from_str(hex)?;
        let pubkey = LxPublicKey::from(inner);
        Ok(pubkey)
    }
}

impl Serialize for LxPublicKey {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        // Use PublicKey's LowerHex impl
        let hex_str = format!("{:x}", self.0);
        serializer.serialize_str(&hex_str)
    }
}

impl<'de> Deserialize<'de> for LxPublicKey {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        deserializer.deserialize_str(LxPublicKeyVisitor)
    }
}

struct LxPublicKeyVisitor;

impl<'de> de::Visitor<'de> for LxPublicKeyVisitor {
    type Value = LxPublicKey;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("lower hex-encoded LxPublicKey")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        LxPublicKey::from_str(v).map_err(de::Error::custom)
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

    pub fn to_str(self) -> &'static str {
        match self.into_inner() {
            bitcoin::Network::Bitcoin => "main",
            bitcoin::Network::Testnet => "test",
            bitcoin::Network::Regtest => "regtest",
            bitcoin::Network::Signet => "signet",
        }
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

impl From<Network> for bitcoin_bech32::constants::Network {
    fn from(network: Network) -> Self {
        match network.into_inner() {
            bitcoin::Network::Bitcoin => {
                bitcoin_bech32::constants::Network::Bitcoin
            }
            bitcoin::Network::Testnet => {
                bitcoin_bech32::constants::Network::Testnet
            }
            bitcoin::Network::Regtest => {
                bitcoin_bech32::constants::Network::Regtest
            }
            bitcoin::Network::Signet => {
                bitcoin_bech32::constants::Network::Signet
            }
        }
    }
}

impl From<Network> for Currency {
    fn from(network: Network) -> Self {
        match network.into_inner() {
            bitcoin::Network::Bitcoin => Currency::Bitcoin,
            bitcoin::Network::Testnet => Currency::BitcoinTestnet,
            bitcoin::Network::Regtest => Currency::Regtest,
            bitcoin::Network::Signet => Currency::Signet,
        }
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
    use crate::lexe::bitcoind::BitcoindRpcInfo;

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

    #[test]
    fn pubkey_serde() {
        let input = "02692f6894d5cb51bb785cc3c54f457889faf674fedea54a906f7ec99e88832d18";
        // JSON requires double quotes
        let input_json = format!("\"{input}\"");

        // Check that FromStr and Deserialize produce the same result
        let pubkey1 = LxPublicKey::from_str(input).unwrap();
        let pubkey2: LxPublicKey = serde_json::from_str(&input_json).unwrap();
        assert_eq!(pubkey1, pubkey2);

        // Serialize both to json again
        let output_json1 = serde_json::to_string(&pubkey1).unwrap();
        let output_json2 = serde_json::to_string(&pubkey2).unwrap();
        assert_eq!(output_json1, output_json2);
        assert_eq!(input_json, output_json1);
        assert_eq!(input_json, output_json2);
    }
}
