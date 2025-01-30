use serde::{Deserialize, Serialize};

use super::user::NodePk;
use crate::time::TimestampMs;

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
