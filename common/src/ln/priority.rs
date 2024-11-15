use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::anyhow;
use lightning::chain::chaininterface::ConfirmationTarget;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// Small extension trait to ease interop between our [`ConfirmationPriority`]
/// and LDK's [`ConfirmationTarget`].
pub trait ToNumBlocks {
    /// Convert a confirmation priority into a target number of blocks.
    fn to_num_blocks(&self) -> u16;
}

impl ToNumBlocks for ConfirmationTarget {
    fn to_num_blocks(&self) -> u16 {
        // Based on ldk-node's FeeEstimator implementation.
        match self {
            ConfirmationTarget::MaximumFeeEstimate => 1,
            ConfirmationTarget::UrgentOnChainSweep => 6,
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee => 1008,
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee => 144,
            ConfirmationTarget::AnchorChannelFee => 1008,
            ConfirmationTarget::NonAnchorChannelFee => 12,
            ConfirmationTarget::ChannelCloseMinimum => 144,
            ConfirmationTarget::OutputSpendingFee => 12,
        }
    }
}

/// The transaction confirmation priority levels used in Lexe APIs.
/// Basically a simplified version of LDK's [`ConfirmationTarget`] type.
/// Lexe code should prefer to use this type when possible.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum ConfirmationPriority {
    High,
    Normal,
    Background,
}

impl ToNumBlocks for ConfirmationPriority {
    fn to_num_blocks(&self) -> u16 {
        match self {
            ConfirmationPriority::High => 1,
            ConfirmationPriority::Normal => 3,
            ConfirmationPriority::Background => 72,
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
