use std::{fmt, fmt::Display, str::FromStr};

use anyhow::anyhow;
use bitcoin::{
    blockdata::constants::{self, ChainHash},
    hash_types::BlockHash,
};
use bitcoin_hashes::Hash;
use lightning_invoice::Currency;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::Serialize;
use serde_with::DeserializeFromStr;
use strum::VariantArray;

/// A simple version of [`bitcoin::Network`] which impls [`FromStr`] and
/// [`Display`] in a consistent way, and which isn't `#[non_exhaustive]`.
///
/// There are slight variations in how the network is represented as strings
/// across bitcoin, lightning, Lexe, etc. For consistency, we use the mapping
/// defined in [`bitcoin::Network`]'s `FromStr` impl, which is:
///
/// - Bitcoin <-> "bitcoin"
/// - Testnet <-> "testnet",
/// - Signet <-> "signet",
/// - Regtest <-> "regtest"
#[derive(Copy, Clone, Debug, Eq, PartialEq, DeserializeFromStr, VariantArray)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxNetwork {
    Mainnet,
    Testnet,
    Regtest,
    Signet,
}

impl LxNetwork {
    /// Convert to a [`bitcoin::Network`].
    /// Equivalent to using the [`From`] impl.
    #[inline]
    pub fn to_bitcoin(self) -> bitcoin::Network {
        bitcoin::Network::from(self)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "bitcoin",
            Self::Testnet => "testnet",
            Self::Regtest => "regtest",
            Self::Signet => "signet",
        }
    }

    /// Gets the [`BlockHash`] of the genesis block for this [`LxNetwork`].
    pub fn genesis_block_hash(self) -> BlockHash {
        let chain_hash = Self::genesis_chain_hash(self);
        let hash =
            bitcoin::hashes::sha256d::Hash::from_inner(chain_hash.into_bytes());
        BlockHash::from_hash(hash)
    }

    /// Gets the block hash of the genesis block for this [`LxNetwork`], but
    /// returns the other [`ChainHash`] newtype.
    #[inline]
    pub fn genesis_chain_hash(self) -> ChainHash {
        constants::ChainHash::using_genesis_block(self.to_bitcoin())
    }
}

impl FromStr for LxNetwork {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bitcoin" => Ok(Self::Mainnet),
            "testnet" => Ok(Self::Testnet),
            "regtest" => Ok(Self::Regtest),
            "signet" => Ok(Self::Signet),
            _ => Err(anyhow!(
                "`LxNetwork` must be one of: \
                 ['bitcoin', 'testnet', 'regtest', 'signet']"
            )),
        }
    }
}

impl Display for LxNetwork {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<bitcoin::Network> for LxNetwork {
    type Error = anyhow::Error;
    #[inline]
    fn try_from(bitcoin: bitcoin::Network) -> Result<Self, Self::Error> {
        match bitcoin {
            bitcoin::Network::Bitcoin => Ok(Self::Mainnet),
            bitcoin::Network::Testnet => Ok(Self::Testnet),
            bitcoin::Network::Signet => Ok(Self::Signet),
            bitcoin::Network::Regtest => Ok(Self::Regtest),
        }
    }
}

impl From<LxNetwork> for bitcoin::Network {
    fn from(lx: LxNetwork) -> Self {
        match lx {
            LxNetwork::Mainnet => Self::Bitcoin,
            LxNetwork::Testnet => Self::Testnet,
            LxNetwork::Regtest => Self::Regtest,
            LxNetwork::Signet => Self::Signet,
        }
    }
}

impl From<LxNetwork> for bitcoin_bech32::constants::Network {
    fn from(lx: LxNetwork) -> Self {
        match lx {
            LxNetwork::Mainnet => Self::Bitcoin,
            LxNetwork::Testnet => Self::Testnet,
            LxNetwork::Regtest => Self::Regtest,
            LxNetwork::Signet => Self::Signet,
        }
    }
}

impl From<LxNetwork> for Currency {
    fn from(lx: LxNetwork) -> Self {
        match lx {
            LxNetwork::Mainnet => Self::Bitcoin,
            LxNetwork::Testnet => Self::BitcoinTestnet,
            LxNetwork::Regtest => Self::Regtest,
            LxNetwork::Signet => Self::Signet,
        }
    }
}

impl Serialize for LxNetwork {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.as_str().serialize(serializer)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn network_roundtrip() {
        let expected_ser = r#"["bitcoin","testnet","regtest","signet"]"#;
        roundtrip::json_unit_enum_backwards_compat::<LxNetwork>(expected_ser);
        roundtrip::fromstr_display_roundtrip_proptest::<LxNetwork>();
    }

    // Sanity check that Hash(genesis_block) == precomputed hash
    #[test]
    fn check_precomputed_genesis_block_hashes() {
        for network in LxNetwork::VARIANTS {
            let precomputed = network.genesis_block_hash();
            let computed = constants::genesis_block(network.to_bitcoin())
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
        let actual = LxNetwork::Mainnet.genesis_chain_hash();
        assert_eq!(actual.as_bytes().as_slice(), expected.as_slice());
    }
}
