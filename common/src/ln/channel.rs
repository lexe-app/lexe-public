use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::Context;
use lightning::chain::transaction::OutPoint;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;

use crate::ln::hashes::LxTxid;

/// A newtype for [`OutPoint`] that provides [`FromStr`] / [`Display`] impls.
///
/// Since the persister relies on the string representation to identify
/// channels, having a newtype (instead of upstreaming these impls to LDK)
/// ensures that the serialization scheme does not change from beneath us.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxOutPoint {
    pub txid: LxTxid,
    pub index: u16,
}

impl From<OutPoint> for LxOutPoint {
    fn from(op: OutPoint) -> Self {
        Self {
            txid: LxTxid(op.txid),
            index: op.index,
        }
    }
}

impl From<LxOutPoint> for OutPoint {
    fn from(op: LxOutPoint) -> Self {
        Self {
            txid: op.txid.0,
            index: op.index,
        }
    }
}

/// Deserializes from `<txid>_<index>`
impl FromStr for LxOutPoint {
    type Err = anyhow::Error;
    fn from_str(outpoint_str: &str) -> anyhow::Result<Self> {
        let mut txid_and_txindex = outpoint_str.split('_');
        let txid_str = txid_and_txindex
            .next()
            .context("Missing <txid> in <txid>_<index>")?;
        let index_str = txid_and_txindex
            .next()
            .context("Missing <index> in <txid>_<index>")?;

        let txid = LxTxid::from_str(txid_str)
            .context("Invalid txid returned from DB")?;
        let index = u16::from_str(index_str)
            .context("Could not parse index into u16")?;

        Ok(Self { txid, index })
    }
}

/// Serializes to `<txid>_<index>`
impl Display for LxOutPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}_{}", self.txid, self.index)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;
    #[test]
    fn outpoint_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxOutPoint>();
    }
}
