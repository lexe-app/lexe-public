use std::fmt::{self, Display};
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::{ensure, Context};
use bitcoin::blockdata::constants;
use bitcoin::hash_types::BlockHash;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::RouteHintHop;
use lightning_invoice::Currency;
use serde::{Deserialize, Serialize};

use crate::api::{NodePk, Scid};
use crate::ln::peer::ChannelPeer;

/// User node CLI args.
pub mod node;

pub const MAINNET_NETWORK: Network = Network(bitcoin::Network::Bitcoin);
pub const TESTNET_NETWORK: Network = Network(bitcoin::Network::Testnet);
pub const REGTEST_NETWORK: Network = Network(bitcoin::Network::Regtest);
pub const SIGNET_NETWORK: Network = Network(bitcoin::Network::Signet);

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
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Network(pub bitcoin::Network);

/// Information about the LSP which the user node needs to connect and to
/// generate route hints when no channel exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspInfo {
    /// The protocol://host:port of the LSP's HTTP server. The node will
    /// default to a mock client if not supplied, provided that
    /// `--allow-mock` is set and we are not in prod.
    pub url: Option<String>,
    // - ChannelPeer fields - //
    pub node_pk: NodePk,
    /// The socket on which the LSP accepts P2P LN connections from user nodes
    pub addr: SocketAddr,
    // - RoutingFees fields - //
    pub base_msat: u32,
    pub proportional_millionths: u32,
    // - RouteHintHop fields - //
    pub cltv_expiry_delta: u16,
    pub htlc_minimum_msat: u64,
    pub htlc_maximum_msat: u64,
}

// --- impl Network --- //

impl Network {
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

    /// Gets the blockhash of the genesis block of this [`Network`]
    pub fn genesis_hash(self) -> BlockHash {
        constants::genesis_block(self.to_inner())
            .header
            .block_hash()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for Network {
    fn default() -> Self {
        Self(bitcoin::Network::Regtest)
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

impl Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl From<Network> for bitcoin_bech32::constants::Network {
    fn from(network: Network) -> Self {
        match network.to_inner() {
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
            addr: self.addr,
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

// --- Arbitrary impls --- //

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary {
    use proptest::arbitrary::Arbitrary;
    use proptest::strategy::{BoxedStrategy, Just, Strategy};

    use super::*;

    impl Arbitrary for Network {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            proptest::prop_oneof! {
                // TODO: Mainnet is disabled for now
                // Just(Network(bitcoin::Network::Bitcoin)),
                Just(Network(bitcoin::Network::Testnet)),
                Just(Network(bitcoin::Network::Regtest)),
                Just(Network(bitcoin::Network::Signet)),
            }
            .boxed()
        }
    }
}

#[cfg(all(any(test, feature = "test-utils"), not(target_env = "sgx")))]
mod arbitrary_not_sgx {
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;
    use crate::test_utils::arbitrary;

    impl Arbitrary for LspInfo {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (
                any::<Option<String>>(),
                any::<NodePk>(),
                arbitrary::any_socket_addr(),
                any::<u32>(),
                any::<u32>(),
                any::<u16>(),
                any::<u64>(),
                any::<u64>(),
            )
                .prop_map(
                    |(
                        url,
                        node_pk,
                        addr,
                        base_msat,
                        proportional_millionths,
                        cltv_expiry_delta,
                        htlc_minimum_msat,
                        htlc_maximum_msat,
                    )| {
                        Self {
                            url,
                            node_pk,
                            addr,
                            base_msat,
                            proportional_millionths,
                            cltv_expiry_delta,
                            htlc_minimum_msat,
                            htlc_maximum_msat,
                        }
                    },
                )
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
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test_notsgx {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn lsp_info_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LspInfo>();
    }
}
