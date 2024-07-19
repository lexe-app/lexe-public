use std::{fmt, fmt::Display, path::Path, process::Command, str::FromStr};

use anyhow::Context;
use bitcoin::{
    blockdata::constants::{self, ChainHash},
    hash_types::BlockHash,
};
use bitcoin_hashes::Hash;
use lightning::routing::{gossip::RoutingFees, router::RouteHintHop};
use lightning_invoice::Currency;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::{NodePk, Scid},
    ln::{addr::LxSocketAddress, peer::ChannelPeer},
};

/// User node CLI args.
pub mod node;

/// A trait for converting CLI args to [`Command`]s.
pub trait ToCommand {
    /// Construct a [`Command`] from the contained args.
    /// Requires the path to the binary.
    fn to_command(&self, bin_path: &Path) -> Command {
        let mut command = Command::new(bin_path);
        self.append_args(&mut command);
        command
    }

    /// Serialize and append the contained args to an existing [`Command`].
    fn append_args(&self, cmd: &mut Command);
}

/// A wrapper around [`bitcoin::Network`] that implements [`FromStr`] /
/// [`Display`] in a consistent way.
///
/// There are slight variations is how the network is represented as strings
/// across bitcoin, lightning, Lexe, etc. For consistency, we use the mapping
/// defined in [`bitcoin::Network`]'s `FromStr` impl, which is:
///
/// - Bitcoin <-> "bitcoin"
/// - Testnet <-> "testnet",
/// - Signet <-> "signet",
/// - Regtest <-> "regtest"
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Network(pub bitcoin::Network);

/// Information about the LSP which the user node needs to connect and to
/// generate route hints when no channel exists.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LspInfo {
    /// The protocol://host:port of the LSP's HTTP server. The node will
    /// default to a mock client if not supplied, provided that
    /// `--allow-mock` is set and we are not in prod.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub url: Option<String>,
    // - ChannelPeer fields - //
    pub node_pk: NodePk,
    /// The socket on which the LSP accepts P2P LN connections from user nodes
    pub addr: LxSocketAddress,
    // - RoutingFees fields - //
    pub base_msat: u32,
    pub proportional_millionths: u32,
    // - RouteHintHop fields - //
    pub cltv_expiry_delta: u16,
    pub htlc_minimum_msat: u64,
    pub htlc_maximum_msat: u64,
}

/// Configuration info relating to Google OAuth2. When combined with an auth
/// `code`, can be used to obtain a GDrive access token and refresh token.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct OAuthConfig {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_id: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_secret: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub redirect_uri: String,
}

// --- impl Network --- //

impl Network {
    pub const MAINNET: Self = Self(bitcoin::Network::Bitcoin);
    pub const TESTNET: Self = Self(bitcoin::Network::Testnet);
    pub const REGTEST: Self = Self(bitcoin::Network::Regtest);
    pub const SIGNET: Self = Self(bitcoin::Network::Signet);

    pub const ALL: [Self; 4] =
        [Self::MAINNET, Self::TESTNET, Self::REGTEST, Self::SIGNET];

    #[inline]
    pub fn to_inner(self) -> bitcoin::Network {
        self.0
    }

    pub fn to_str(self) -> &'static str {
        match self.to_inner() {
            bitcoin::Network::Bitcoin => "bitcoin",
            bitcoin::Network::Testnet => "testnet",
            bitcoin::Network::Regtest => "regtest",
            bitcoin::Network::Signet => "signet",
        }
    }

    /// Gets the [`BlockHash`] of the genesis block for this [`Network`].
    pub fn genesis_block_hash(self) -> BlockHash {
        let chain_hash = Self::genesis_chain_hash(self);
        let hash =
            bitcoin::hashes::sha256d::Hash::from_inner(chain_hash.into_bytes());
        BlockHash::from_hash(hash)
    }

    /// Gets the block hash of the genesis block for this [`Network`], but
    /// returns the other [`ChainHash`] newtype.
    #[inline]
    pub fn genesis_chain_hash(self) -> ChainHash {
        constants::ChainHash::using_genesis_block(self.to_inner())
    }
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        bitcoin::Network::from_str(s)
            .map(Self)
            .context("Failed to parse bitcoin::Network")
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl From<Network> for bitcoin_bech32::constants::Network {
    fn from(network: Network) -> Self {
        match network.to_inner() {
            bitcoin::Network::Bitcoin =>
                bitcoin_bech32::constants::Network::Bitcoin,
            bitcoin::Network::Testnet =>
                bitcoin_bech32::constants::Network::Testnet,
            bitcoin::Network::Regtest =>
                bitcoin_bech32::constants::Network::Regtest,
            bitcoin::Network::Signet =>
                bitcoin_bech32::constants::Network::Signet,
        }
    }
}

impl From<Network> for Currency {
    fn from(network: Network) -> Self {
        match network.to_inner() {
            bitcoin::Network::Bitcoin => Currency::Bitcoin,
            bitcoin::Network::Testnet => Currency::BitcoinTestnet,
            bitcoin::Network::Regtest => Currency::Regtest,
            bitcoin::Network::Signet => Currency::Signet,
        }
    }
}

// --- impl LspInfo --- //

impl LspInfo {
    pub fn channel_peer(&self) -> ChannelPeer {
        ChannelPeer {
            node_pk: self.node_pk,
            addr: self.addr.clone(),
        }
    }

    pub fn route_hint_hop(&self, scid: Scid) -> RouteHintHop {
        RouteHintHop {
            src_node_id: self.node_pk.0,
            short_channel_id: scid.0,
            fees: RoutingFees {
                base_msat: self.base_msat,
                proportional_millionths: self.proportional_millionths,
            },
            cltv_expiry_delta: self.cltv_expiry_delta,
            htlc_minimum_msat: Some(self.htlc_minimum_msat),
            htlc_maximum_msat: Some(self.htlc_maximum_msat),
        }
    }

    /// Returns a dummy [`LspInfo`] which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn dummy() -> Self {
        use std::net::Ipv6Addr;

        use crate::{rng::WeakRng, root_seed::RootSeed, test_utils};

        let mut rng = WeakRng::from_u64(20230216);
        let node_pk = RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng);
        let addr = LxSocketAddress::TcpIpv6 {
            ip: Ipv6Addr::LOCALHOST,
            port: 42069,
        };

        Self {
            url: Some(test_utils::DUMMY_LSP_URL.to_owned()),
            node_pk,
            addr,
            base_msat: 0,
            proportional_millionths: 3000,
            cltv_expiry_delta: 72,
            htlc_minimum_msat: 1,
            htlc_maximum_msat: u64::MAX,
        }
    }
}

impl FromStr for LspInfo {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).context("Invalid JSON")
    }
}

impl Display for LspInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json_str = serde_json::to_string(&self)
            .expect("Does not contain map with non-string keys");
        write!(f, "{json_str}")
    }
}

// --- impl OAuthConfig --- //

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let client_id = &self.client_id;
        let redirect_uri = &self.redirect_uri;
        write!(
            f,
            "OAuthConfig {{ \
                client_id: {client_id}, \
                redirect_uri: {redirect_uri}, \
                .. \
            }}"
        )
    }
}

impl FromStr for OAuthConfig {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).context("Invalid JSON")
    }
}

impl Display for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json_str = serde_json::to_string(&self)
            .expect("Does not contain map with non-string keys");
        write!(f, "{json_str}")
    }
}

// --- Arbitrary impls --- //

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impls {
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;

    impl Arbitrary for Network {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            proptest::prop_oneof! {
                Just(Network(bitcoin::Network::Bitcoin)),
                Just(Network(bitcoin::Network::Testnet)),
                Just(Network(bitcoin::Network::Regtest)),
                Just(Network(bitcoin::Network::Signet)),
            }
            .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn network_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<Network>();
    }

    #[test]
    fn lsp_info_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<LspInfo>();
        roundtrip::fromstr_display_roundtrip_proptest::<LspInfo>();
    }

    #[test]
    fn oauth_config_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<OAuthConfig>();
        roundtrip::fromstr_display_roundtrip_proptest::<OAuthConfig>();
    }

    // Sanity check that Hash(genesis_block) == precomputed hash
    #[test]
    fn check_precomputed_genesis_block_hashes() {
        for network in Network::ALL {
            let precomputed = network.genesis_block_hash();
            let computed = constants::genesis_block(network.to_inner())
                .header
                .block_hash();
            assert_eq!(precomputed, computed);
        }
    }

    // Sanity check mainnet genesis block hash
    #[test]
    fn absolutely_check_mainnet_genesis_hash() {
        let expected = hex::decode(
            "6fe28c0ab6f1b372c1a6a246ae63f74f931e8365e15a089c68d6190000000000",
        )
        .unwrap();
        let actual = Network::MAINNET.genesis_chain_hash();
        assert_eq!(actual.as_bytes().as_slice(), expected.as_slice());
    }
}
