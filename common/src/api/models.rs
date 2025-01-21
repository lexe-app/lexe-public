use serde::{Deserialize, Serialize};

use crate::time::TimestampMs;

/// A response to a status check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The current time, according to this service.
    pub timestamp: TimestampMs,
    // TODO(max): We can add more metrics here, like CPU and memory usage (if
    // available within SGX), # of tasks, etc.
}
