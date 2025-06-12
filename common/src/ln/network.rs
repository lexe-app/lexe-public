use std::{fmt, fmt::Display, str::FromStr};

use anyhow::anyhow;
use bitcoin::{
    blockdata::constants::{self, ChainHash},
    hash_types::BlockHash,
    hashes::Hash as _,
};
use lightning_invoice::Currency;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::Serialize;
use serde_with::DeserializeFromStr;
use strum::VariantArray;

/// A simple version of [`bitcoin::Network`] which impls [`FromStr`] and
/// [`Display`] in a consistent way, and which isn't `#[non_exhaustive]`.
///
/// NOTE: [`bitcoin::Network`] serializes their mainnet variant as "bitcoin",
/// while we serialize it as "mainnet". Be sure to use *our* [`serde`] impls
/// when (de)serializing this network.
#[derive(Copy, Clone, Debug, Eq, PartialEq, DeserializeFromStr, VariantArray)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxNetwork {
    Mainnet,
    Testnet3,
    Testnet4,
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
            Self::Mainnet => "mainnet",
            Self::Testnet3 => "testnet3",
            Self::Testnet4 => "testnet4",
            Self::Regtest => "regtest",
            Self::Signet => "signet",
        }
    }

    /// Gets the [`BlockHash`] of the genesis block for this [`LxNetwork`].
    pub fn genesis_block_hash(self) -> BlockHash {
        let chain_hash = Self::genesis_chain_hash(self);
        BlockHash::from_byte_array(chain_hash.to_bytes())
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
            "mainnet" => Ok(Self::Mainnet),
            "testnet3" => Ok(Self::Testnet3),
            "testnet4" => Ok(Self::Testnet4),
            "regtest" => Ok(Self::Regtest),
            "signet" => Ok(Self::Signet),
            _ => Err(anyhow!("Invalid `LxNetwork`")),
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

    fn try_from(network: bitcoin::Network) -> Result<Self, Self::Error> {
        let maybe_network = match network {
            bitcoin::Network::Bitcoin => Some(Self::Mainnet),
            bitcoin::Network::Testnet => Some(Self::Testnet3),
            bitcoin::Network::Testnet4 => Some(Self::Testnet4),
            bitcoin::Network::Signet => Some(Self::Signet),
            bitcoin::Network::Regtest => Some(Self::Regtest),
            _ => None,
        };

        debug_assert!(
            maybe_network.is_some(),
            "We're missing a bitcoin::Network variant"
        );

        maybe_network
            .ok_or_else(|| anyhow!("Unknown `bitcoin::Network`: {network:?}"))
    }
}

impl From<LxNetwork> for bitcoin::Network {
    fn from(lx: LxNetwork) -> Self {
        match lx {
            LxNetwork::Mainnet => Self::Bitcoin,
            LxNetwork::Testnet3 => Self::Testnet,
            LxNetwork::Testnet4 => Self::Testnet4,
            LxNetwork::Regtest => Self::Regtest,
            LxNetwork::Signet => Self::Signet,
        }
    }
}

impl From<LxNetwork> for Currency {
    fn from(lx: LxNetwork) -> Self {
        match lx {
            LxNetwork::Mainnet => Self::Bitcoin,
            LxNetwork::Testnet3 | LxNetwork::Testnet4 => Self::BitcoinTestnet,
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
        let expected_ser =
            r#"["mainnet","testnet3","testnet4","regtest","signet"]"#;
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
