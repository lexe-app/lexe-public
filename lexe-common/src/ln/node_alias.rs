use std::fmt::{self, Write};

use lightning::{routing::gossip::NodeAlias, util::string::PrintableString};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::serde_helpers::hexstr_or_bytes;

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
        let bytes = &self.0;
        let trimmed = bytes.split(|&b| b == 0).next().unwrap_or(bytes);

        // This block is basically copied from `NodeAlias`'s `Display` impl.
        // - If bytes are valid UTF-8, display its printable characters.
        // - If bytes are not UTF-8, display its printable ASCII characters.
        match std::str::from_utf8(trimmed) {
            Ok(s) => PrintableString(s).fmt(f)?,
            Err(_) =>
                for c in trimmed.iter().map(|b| *b as char) {
                    if ('\x20'..='\x7e').contains(&c) {
                        f.write_char(c)?;
                    } else {
                        f.write_char(char::REPLACEMENT_CHARACTER)?;
                    }
                },
        }

        Ok(())
    }
}
