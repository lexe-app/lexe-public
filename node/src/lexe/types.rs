//! Shared Bitcoin / Lightning Lexe newtypes.

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use bitcoin::hash_types::Txid;
use bitcoin::hashes::sha256d;
use bitcoin::secp256k1::PublicKey;
use common::hex;
use lightning::chain::transaction::OutPoint;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

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

#[cfg(test)]
mod test {
    use super::*;

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
