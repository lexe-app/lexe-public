use std::fmt::{self, Display};

use lightning::ln::{PaymentPreimage, PaymentSecret};

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: MillisatAmount,
}

#[allow(dead_code)]
pub enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

// TODO(max): This struct doesn't seem important - perhaps it can be removed?
pub struct MillisatAmount(pub Option<u64>);

impl Display for MillisatAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(amt) => write!(f, "{amt}"),
            None => write!(f, "unknown"),
        }
    }
}
