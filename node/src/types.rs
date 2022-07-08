//! Core types and data structures used throughout the lexe-node.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{ensure, format_err};
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
use secrecy::{ExposeSecret, Secret, SecretVec};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use subtle::ConstantTimeEq;

use crate::bitcoind_client::BitcoindClient;
use crate::logger::StdOutLogger;
use crate::persister::PostgresPersister;
use crate::{ed25519, hex};

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

pub type BroadcasterType = BitcoindClient;
pub type FeeEstimatorType = BitcoindClient;

pub type LoggerType = Arc<StdOutLogger>;

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

/// The user's root seed from which we derive all child secrets.
pub struct RootSeed(Secret<[u8; Self::LENGTH]>);

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
            network == bitcoin::Network::Testnet,
            "only support testnet for now"
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

// -- impl RootSeed -- //

// TODO(phlip9): zeroize on drop

impl RootSeed {
    pub const LENGTH: usize = 32;

    /// An HKDF can't extract more than `255 * hash_output_size` bytes for a
    /// single secret.
    const HKDF_MAX_OUT_LEN: usize = 8160 /* 255*32 */;

    /// The HKDF domain separation value as a human-readable byte string.
    #[cfg(test)]
    const HKDF_SALT_STR: &'static [u8] = b"LEXE-HASH-REALM::RootSeed";

    /// We salt the HKDF for domain separation purposes. The raw bytes here are
    /// equal to the hash value: `SHA-256(b"LEXE-HASH-REALM::RootSeed")`.
    const HKDF_SALT: [u8; 32] = [
        0x36, 0x3b, 0x11, 0x6b, 0xe1, 0x69, 0x0f, 0xcd, 0x48, 0x1f, 0x2d, 0x40,
        0x14, 0x81, 0x2a, 0xae, 0xcf, 0xf2, 0x41, 0x1b, 0x86, 0x11, 0x98, 0xee,
        0xc4, 0x2c, 0x6e, 0x31, 0xd8, 0x0a, 0x28, 0xa4,
    ];

    pub fn new(bytes: Secret<[u8; Self::LENGTH]>) -> Self {
        Self(bytes)
    }

    fn extract(&self) -> ring::hkdf::Prk {
        let salted_hkdf = ring::hkdf::Salt::new(
            ring::hkdf::HKDF_SHA256,
            Self::HKDF_SALT.as_slice(),
        );
        salted_hkdf.extract(self.0.expose_secret().as_slice())
    }

    /// Derive a new child secret with `label` into a prepared buffer `out`.
    pub fn derive_to_slice(&self, label: &[u8], out: &mut [u8]) {
        struct OkmLength(usize);

        impl ring::hkdf::KeyType for OkmLength {
            fn len(&self) -> usize {
                self.0
            }
        }

        assert!(out.len() <= Self::HKDF_MAX_OUT_LEN);

        let label = &[label];

        self.extract()
            .expand(label, OkmLength(out.len()))
            .expect("should not fail")
            .fill(out)
            .expect("should not fail")
    }

    /// Derive a new child secret with `label` to a hash-output-sized buffer.
    pub fn derive(&self, label: &[u8]) -> Secret<[u8; 32]> {
        let mut out = [0u8; 32];
        self.derive_to_slice(label, &mut out);
        Secret::new(out)
    }

    /// Convenience method to derive a new child secret with `label` into a
    /// `Vec<u8>` of size `out_len`.
    pub fn derive_vec(&self, label: &[u8], out_len: usize) -> SecretVec<u8> {
        let mut out = vec![0u8; out_len];
        self.derive_to_slice(label, &mut out);
        SecretVec::new(out)
    }

    /// Derive the CA cert that endorses client and node certs. These certs
    /// provide mutual authentication for client <-> node connections.
    pub fn derive_client_ca_key_pair(&self) -> rcgen::KeyPair {
        let seed = self.derive(b"client ca key pair");
        ed25519::from_seed(seed.expose_secret())
    }

    #[cfg(test)]
    fn as_bytes(&self) -> &[u8] {
        self.0.expose_secret().as_slice()
    }
}

impl FromStr for RootSeed {
    type Err = hex::DecodeError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; Self::LENGTH];
        hex::decode_to_slice_ct(hex, bytes.as_mut_slice())
            .map(|()| Self::new(Secret::new(bytes)))
    }
}

impl fmt::Debug for RootSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        f.write_str("RootSeed(..)")
    }
}

impl TryFrom<&[u8]> for RootSeed {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::LENGTH {
            return Err(format_err!("input must be {} bytes", Self::LENGTH));
        }
        let mut out = [0u8; Self::LENGTH];
        out[..].copy_from_slice(bytes);
        Ok(Self::new(Secret::new(out)))
    }
}

struct RootSeedVisitor;

impl<'de> de::Visitor<'de> for RootSeedVisitor {
    type Value = RootSeed;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("hex-encoded RootSeed or raw bytes")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        RootSeed::from_str(v).map_err(serde::de::Error::custom)
    }

    fn visit_bytes<E>(self, b: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        RootSeed::try_from(b).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RootSeed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(RootSeedVisitor)
        } else {
            deserializer.deserialize_bytes(RootSeedVisitor)
        }
    }
}

impl Serialize for RootSeed {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            let hex_str = hex::encode(self.0.expose_secret());
            serializer.serialize_str(&hex_str)
        } else {
            serializer.serialize_bytes(self.0.expose_secret())
        }
    }
}

#[cfg(test)]
impl proptest::arbitrary::Arbitrary for RootSeed {
    type Strategy = proptest::strategy::BoxedStrategy<Self>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;

        proptest::arbitrary::any::<[u8; 32]>()
            .prop_map(|buf| Self::new(Secret::new(buf)))
            .boxed()
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::collection::vec;
    use proptest::proptest;
    use ring::digest::Digest;

    use super::*;

    // simple implementations of some crypto functions for equivalence testing

    fn sha256(input: &[u8]) -> Digest {
        ring::digest::digest(&ring::digest::SHA256, input)
    }

    // an inefficient impl of HMAC-SHA256 for equivalence testing
    fn hmac_sha256(key: &[u8], msg: &[u8]) -> Digest {
        let h_key = sha256(key);
        let mut zero_pad_key = [0u8; 64];

        // make key match the internal block size
        let key = match key.len() {
            len if len > 64 => h_key.as_ref(),
            _ => key,
        };
        zero_pad_key[..key.len()].copy_from_slice(key);
        let key = zero_pad_key.as_slice();
        assert_eq!(key.len(), 64);

        // o_key := [ key_i ^ 0x5c ]_{i in 0..64}
        let mut o_key = [0u8; 64];
        for (o_key_i, key_i) in o_key.iter_mut().zip(key) {
            *o_key_i = key_i ^ 0x5c;
        }

        // i_key := [ key_i ^ 0x36 ]_{i in 0..64}
        let mut i_key = [0u8; 64];
        for (i_key_i, key_i) in i_key.iter_mut().zip(key) {
            *i_key_i = key_i ^ 0x36;
        }

        // m_i := i_key || msg
        let mut m_i = i_key.to_vec();
        m_i.extend_from_slice(msg);

        let h_i = sha256(&m_i);

        // m_o := o_key || H(m_i)
        let mut m_o = o_key.to_vec();
        m_o.extend_from_slice(h_i.as_ref());

        // output := H(o_key || H(i_key || msg))
        sha256(&m_o)
    }

    // an inefficient impl of HKDF-SHA256 for equivalence testing
    fn hkdf_sha256(
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        out_len: usize,
    ) -> Vec<u8> {
        let prk = hmac_sha256(salt, ikm);

        // N := ceil(out_len / block_size)
        //   := (out_len.saturating_sub(1) / block_size) + 1
        let n = (out_len.saturating_sub(1) / 32) + 1;
        let n = u8::try_from(n).expect("out_len too large");

        // T := T(1) | T(2) | .. | T(N)
        // T(0) := b"" (empty byte string)
        // T(i+1) := hmac_sha256(prk, T(i) || info || [ i+1 ])

        let mut t_i = [0u8; 32];
        let mut out = Vec::new();

        for i in 1..=n {
            // m_i := T(i-1) || info || [ i ]
            let mut m_i = if i == 1 { Vec::new() } else { t_i.to_vec() };
            m_i.extend_from_slice(info);
            m_i.extend_from_slice(&[i]);

            let h_i = hmac_sha256(prk.as_ref(), &m_i);
            t_i.copy_from_slice(h_i.as_ref());

            if i < n {
                out.extend_from_slice(&t_i[..]);
            } else {
                let l = 32 - (((n as usize) * 32) - out_len);
                out.extend_from_slice(&t_i[..l]);
            }
        }

        out
    }

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
    fn test_root_seed_serde() {
        let input =
            "7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069";
        let input_json = format!("\"{input}\"");
        let seed_bytes = hex::decode(input).unwrap();

        let seed = RootSeed::from_str(input).unwrap();
        assert_eq!(seed.as_bytes(), &seed_bytes);

        let seed2: RootSeed = serde_json::from_str(&input_json).unwrap();
        assert_eq!(seed2.as_bytes(), &seed_bytes);

        #[derive(Deserialize)]
        struct Foo {
            x: u32,
            seed: RootSeed,
            y: String,
        }

        let foo_json = format!(
            "{{\n\
            \"x\": 123,\n\
            \"seed\": \"{input}\",\n\
            \"y\": \"asdf\"\n\
        }}"
        );

        let foo2: Foo = serde_json::from_str(&foo_json).unwrap();
        assert_eq!(foo2.x, 123);
        assert_eq!(foo2.seed.as_bytes(), &seed_bytes);
        assert_eq!(foo2.y, "asdf");
    }
    #[test]
    fn test_root_seed_hkdf_salt() {
        let actual = RootSeed::HKDF_SALT.as_slice();
        let expected = sha256(RootSeed::HKDF_SALT_STR);

        // // print out salt
        // let hex = hex::encode(expected.as_ref());
        // let (chunks, _) = hex.as_bytes().as_chunks::<2>();
        // for &[hi, lo] in chunks {
        //     let hi = hi as char;
        //     let lo = lo as char;
        //     println!("0x{hi}{lo},");
        // }

        // compare hex encode for easier debugging
        assert_eq!(hex::encode(actual), hex::encode(expected.as_ref()),);
    }

    #[test]
    fn test_root_seed_derive() {
        let seed = RootSeed::new(Secret::new([0x42; 32]));

        let out8 = seed.derive_vec(b"very cool secret", 8);
        let out16 = seed.derive_vec(b"very cool secret", 16);
        let out32 = seed.derive_vec(b"very cool secret", 32);
        let out32_2 = seed.derive(b"very cool secret");

        assert_eq!("49fb6bebcd2acb22", hex::encode(out8.expose_secret()));
        assert_eq!(
            "49fb6bebcd2acb223a802f726bd5159d",
            hex::encode(out16.expose_secret())
        );
        assert_eq!(
            "49fb6bebcd2acb223a802f726bd5159d4c982732c550c698aa0558f95575e8c1",
            hex::encode(out32.expose_secret())
        );
        assert_eq!(out32.expose_secret(), out32_2.expose_secret());
    }

    // Fuzz our KDF against a basic, readable implementation of HKDF-SHA256.
    #[test]
    fn test_root_seed_derive_equiv() {
        let arb_seed = any::<RootSeed>();
        let arb_label = vec(any::<u8>(), 0..=64);
        let arb_len = 0_usize..=1024;

        proptest!(|(seed in arb_seed, label in arb_label, len in arb_len)| {
            let expected = hkdf_sha256(
                seed.as_bytes(),
                RootSeed::HKDF_SALT.as_slice(),
                &label,
                len,
            );

            let actual = seed.derive_vec(&label, len);

            assert_eq!(&expected, actual.expose_secret());
        });
    }
}
