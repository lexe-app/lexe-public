use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::ensure;
use bitcoin::blockdata::constants;
use bitcoin::hash_types::BlockHash;
use lightning_invoice::Currency;
#[cfg(any(test, feature = "test-utils"))]
use proptest::arbitrary::Arbitrary;
#[cfg(any(test, feature = "test-utils"))]
use proptest::strategy::{BoxedStrategy, Just, Strategy};
use serde::{Deserialize, Serialize};

/// User node CLI args.
pub mod node;

pub const MAINNET_NETWORK: Network = Network(bitcoin::Network::Bitcoin);
pub const TESTNET_NETWORK: Network = Network(bitcoin::Network::Testnet);
pub const REGTEST_NETWORK: Network = Network(bitcoin::Network::Regtest);
pub const SIGNET_NETWORK: Network = Network(bitcoin::Network::Signet);

/// There are slight variations is how the network is represented as strings
/// across bitcoin, lightning, Lexe, etc. For consistency, we use the mapping
/// defined in [`bitcoin::Network`]'s `FromStr` impl, which is:
///
/// - Bitcoin <-> "bitcoin"
/// - Testnet <-> "testnet",
/// - Signet <-> "signet",
/// - Regtest <-> "regtest"
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Network(bitcoin::Network);

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

#[cfg(any(test, feature = "test-utils"))]
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_network_roundtrip() {
        // TODO: Mainnet is disabled for now
        // let mainnet1 = Network(bitcoin::Network::Bitcoin);
        let testnet1 = Network(bitcoin::Network::Testnet);
        let regtest1 = Network(bitcoin::Network::Regtest);
        let signet1 = Network(bitcoin::Network::Signet);

        // let mainnet2 = Network::from_str(&mainnet1.to_string()).unwrap();
        let testnet2 = Network::from_str(&testnet1.to_string()).unwrap();
        let regtest2 = Network::from_str(&regtest1.to_string()).unwrap();
        let signet2 = Network::from_str(&signet1.to_string()).unwrap();

        // assert_eq!(mainnet1, mainnet2);
        assert_eq!(testnet1, testnet2);
        assert_eq!(regtest1, regtest2);
        assert_eq!(signet1, signet2);
    }
}
