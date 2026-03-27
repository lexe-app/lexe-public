use std::fmt::{self, Write};

use lexe_serde::hexstr_or_bytes;
use lightning::routing::gossip::NodeAlias;
use lightning_types::string::PrintableString;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// Newtype for [`NodeAlias`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxNodeAlias(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

impl LxNodeAlias {
    pub const fn from_str(s: &str) -> Self {
        let len = s.len();
        let input = s.as_bytes();

        debug_assert!(s.len() <= 32);

        let mut out = [0u8; 32];
        let mut idx = 0;
        loop {
            if idx >= len {
                break;
            }
            out[idx] = input[idx];
            idx += 1;
        }

        Self(out)
    }
}

impl From<NodeAlias> for LxNodeAlias {
    fn from(alias: NodeAlias) -> Self {
        Self(alias.0)
    }
}
impl From<LxNodeAlias> for NodeAlias {
    fn from(alias: LxNodeAlias) -> Self {
        Self(alias.0)
    }
}

impl fmt::Debug for LxNodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LxNodeAlias({self})")
    }
}

impl fmt::Display for LxNodeAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This block is basically copied from `NodeAlias`'s `Display` impl.
        // - If bytes are valid UTF-8, display its printable characters.
        // - If bytes are not UTF-8, display its printable ASCII characters.
        let first_null =
            self.0.iter().position(|b| *b == 0).unwrap_or(self.0.len());
        let bytes = self.0.split_at(first_null).0;
        match std::str::from_utf8(bytes) {
            Ok(alias) => PrintableString(alias).fmt(f)?,
            Err(_) =>
                for b in bytes.iter() {
                    let c = if (b'\x20'..=b'\x7e').contains(b) {
                        *b as char
                    } else {
                        char::REPLACEMENT_CHARACTER
                    };
                    f.write_char(c)?;
                },
        }

        Ok(())
    }
}
