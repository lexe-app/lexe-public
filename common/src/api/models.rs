#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use super::user::NodePk;
use crate::{
    ln::hashes::LxTxid, serde_helpers::hexstr_or_bytes, time::TimestampMs,
};

/// A response to a status check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The current time, according to this service.
    pub timestamp: TimestampMs,
    // TODO(max): We can add more metrics here, like CPU and memory usage (if
    // available within SGX), # of tasks, etc.
}

/// A request to sign a message using the node ID secret key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignMsgRequest {
    /// The message to be signed. (Will be signed as UTF-8 bytes.)
    pub msg: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignMsgResponse {
    /// The `zbase32`-encoded signature corresponding to the message.
    pub sig: String,
}

/// A request to verify that a message was signed by the given public key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerifyMsgRequest {
    /// The message to be verified. (Will be interpreted as UTF-8 bytes.)
    pub msg: String,
    /// The `zbase32`-encoded signature corresponding to the message.
    pub sig: String,
    /// The public key under which the signature should be valid.
    pub pk: NodePk,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerifyMsgResponse {
    /// Whether the signature for the message was valid under the given pk.
    pub is_valid: bool,
}

/// The user node or LSP broadcasted an on-chain transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BroadcastedTx {
    /// (PK)
    pub txid: LxTxid,
    /// Consensus-encoded [`bitcoin::Transaction`].
    #[serde(with = "hexstr_or_bytes")]
    pub tx: Vec<u8>,

    /// When this tx was broadcasted.
    pub created_at: TimestampMs,
}

impl BroadcastedTx {
    pub fn new(txid: LxTxid, tx: Vec<u8>) -> Self {
        Self {
            txid,
            tx,
            created_at: TimestampMs::now(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn broadcasted_tx_roundtrip_proptest() {
        roundtrip::json_value_roundtrip_proptest::<BroadcastedTx>();
    }
}
