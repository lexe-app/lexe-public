use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::anyhow;
use lightning::chain::chaininterface::ConfirmationTarget;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// The transaction confirmation priority levels used in Lexe code.
/// A simplified version of LDK's [`ConfirmationTarget`] type.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum ConfirmationPriority {
    High,
    Normal,
    Background,
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

impl From<ConfirmationPriority> for ConfirmationTarget {
    fn from(lexe_newtype: ConfirmationPriority) -> Self {
        match lexe_newtype {
            ConfirmationPriority::High => Self::HighPriority,
            ConfirmationPriority::Normal => Self::Normal,
            ConfirmationPriority::Background => Self::Background,
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
