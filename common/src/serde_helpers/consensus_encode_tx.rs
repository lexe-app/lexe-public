//! [`serde`] helpers to consensus-encode [`bitcoin::Transaction`]s.
//!
//! Serializes transactions as hex strings for human-readable formats, and as
//! raw consensus-encoded bytes for binary formats.
//!
//! ## Example:
//!
//! ```rust
//! use common::serde_helpers::consensus_encode_tx;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo {
//!     #[serde(with = "consensus_encode_tx")]
//!     tx: bitcoin::Transaction,
//! }
//! ```

use std::{borrow::Borrow, fmt};

use bitcoin::consensus;
use serde::{Deserializer, Serializer, de};

pub fn serialize<S, T>(data: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Borrow<bitcoin::Transaction>,
{
    let tx = data.borrow();

    if serializer.is_human_readable() {
        let hex = consensus::encode::serialize_hex(tx);
        serializer.serialize_str(&hex)
    } else {
        let bytes = consensus::encode::serialize(tx);
        serializer.serialize_bytes(&bytes)
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: From<bitcoin::Transaction>,
{
    struct TxVisitor;

    impl de::Visitor<'_> for TxVisitor {
        type Value = bitcoin::Transaction;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a hex-encoded consensus-serialized transaction")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            consensus::encode::deserialize_hex(s).map_err(de::Error::custom)
        }
    }

    struct BytesVisitor;

    impl de::Visitor<'_> for BytesVisitor {
        type Value = bitcoin::Transaction;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("consensus-serialized transaction bytes")
        }

        fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            consensus::encode::deserialize(v).map_err(de::Error::custom)
        }

        fn visit_byte_buf<E: de::Error>(
            self,
            v: Vec<u8>,
        ) -> Result<Self::Value, E> {
            consensus::encode::deserialize(&v).map_err(de::Error::custom)
        }
    }

    let tx = if deserializer.is_human_readable() {
        deserializer.deserialize_str(TxVisitor)?
    } else {
        deserializer.deserialize_bytes(BytesVisitor)?
    };

    Ok(T::from(tx))
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use bitcoin::consensus::Encodable;
    use proptest::{prop_assert_eq, proptest};
    use serde::{Deserialize, Serialize};

    use crate::test_utils::arbitrary;

    #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
    struct TxWrapper {
        #[serde(with = "super")]
        tx: bitcoin::Transaction,
    }

    #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
    struct ArcTxWrapper {
        #[serde(with = "super")]
        tx: Arc<bitcoin::Transaction>,
    }

    /// Sanity check that TxWrapper looks how we expect when JSON-encoded.
    #[test]
    fn json_serialization_basic() {
        use bitcoin::{
            Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
            Txid, Witness, absolute, hashes::Hash, transaction,
        };

        // Build a minimal, deterministic transaction
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([0x42; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(50_000),
                script_pubkey: ScriptBuf::new(),
            }],
        };

        let wrapper = TxWrapper { tx };
        let json = serde_json::to_string(&wrapper).unwrap();

        // tx field is consensus-encoded hex
        let expected = r#"{"tx":"020000000142424242424242424242424242424242424242424242424242424242424242420000000000ffffffff0150c30000000000000000000000"}"#;
        assert_eq!(json, expected);
    }

    /// Verify that JSON serialization produces the consensus-encoded hex.
    #[test]
    fn json_matches_consensus_hex() {
        proptest!(|(tx in arbitrary::any_raw_tx())| {
            // Manually consensus-encode and hex to verify our serde helper
            let expected_hex = {
                let mut bytes = Vec::new();
                tx.consensus_encode(&mut bytes).unwrap();
                hex::encode(&bytes)
            };

            // Test with bitcoin::Transaction
            let wrapper = TxWrapper { tx: tx.clone() };
            let json = serde_json::to_value(&wrapper).unwrap();
            let actual_hex = json["tx"].as_str().unwrap();
            prop_assert_eq!(&expected_hex, actual_hex);

            // Test with Arc<bitcoin::Transaction>
            let arc_wrapper = ArcTxWrapper { tx: Arc::new(tx) };
            let json = serde_json::to_value(&arc_wrapper).unwrap();
            let actual_hex = json["tx"].as_str().unwrap();
            prop_assert_eq!(&expected_hex, actual_hex);
        });
    }

    /// Verify that JSON roundtrips correctly.
    #[test]
    fn json_roundtrip() {
        proptest!(|(tx in arbitrary::any_raw_tx())| {
            let wrapper1 = TxWrapper { tx: tx.clone() };
            let json = serde_json::to_string(&wrapper1).unwrap();
            let wrapper2: TxWrapper = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(wrapper1, wrapper2);

            let arc_wrapper1 = ArcTxWrapper { tx: Arc::new(tx) };
            let json = serde_json::to_string(&arc_wrapper1).unwrap();
            let arc_wrapper2: ArcTxWrapper = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(arc_wrapper1, arc_wrapper2);
        });
    }

    /// Verify that BCS roundtrips correctly.
    #[test]
    fn bcs_roundtrip() {
        proptest!(|(tx in arbitrary::any_raw_tx())| {
            let wrapper1 = TxWrapper { tx: tx.clone() };
            let bytes = bcs::to_bytes(&wrapper1).unwrap();
            let wrapper2: TxWrapper = bcs::from_bytes(&bytes).unwrap();
            prop_assert_eq!(wrapper1, wrapper2);

            let arc_wrapper1 = ArcTxWrapper { tx: Arc::new(tx) };
            let bytes = bcs::to_bytes(&arc_wrapper1).unwrap();
            let arc_wrapper2: ArcTxWrapper = bcs::from_bytes(&bytes).unwrap();
            prop_assert_eq!(arc_wrapper1, arc_wrapper2);
        });
    }
}
