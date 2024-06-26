//! Bitcoin / Lightning Lexe newtypes which have to be in `common` for some
//! reason, likely because they are referenced in an API definition somewhere.

use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::anyhow;
use lightning::chain::chaininterface::ConfirmationTarget;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// `LxSocketAddress`
pub mod addr;
/// `Amount`.
pub mod amount;
/// `AmountOrAll`.
pub mod amount_or_all;
/// `Balance`.
pub mod balance;
/// Channel outpoint, details, counterparty
pub mod channel;
/// Bitcoin hash types, such as `LxTxid`.
pub mod hashes;
/// `LxInvoice`, a wrapper around LDK's BOLT11 invoice type.
pub mod invoice;
/// `LxOffer`, a wrapper around LDK's BOLT12 offer type.
pub mod offer;
/// Payments types and newtypes.
pub mod payments;
/// `ChannelPeer`.
pub mod peer;

/// A newtype for [`ConfirmationTarget`] with [`serde`] and proptest impls.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum ConfirmationPriority {
    High,
    Normal,
    Background,
}

impl From<ConfirmationPriority> for ConfirmationTarget {
    fn from(lexe_newtype: ConfirmationPriority) -> Self {
        match lexe_newtype {
            ConfirmationPriority::High => Self::HighPriority,
            ConfirmationPriority::Normal => Self::Normal,
            ConfirmationPriority::Background => Self::Background,
        }
    }
}

impl FromStr for ConfirmationPriority {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "high" => Ok(Self::High),
            "normal" => Ok(Self::Normal),
            "background" => Ok(Self::Background),
            _ => Err(anyhow!("Must be one of: 'high', 'normal', 'background'")),
        }
    }
}

impl Display for ConfirmationPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Normal => write!(f, "normal"),
            Self::Background => write!(f, "background"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip::json_unit_enum_backwards_compat;

    #[test]
    fn conf_prio_json_backward_compat() {
        let expected_ser = r#"["High","Normal","Background"]"#;
        json_unit_enum_backwards_compat::<ConfirmationPriority>(expected_ser);
    }
}
