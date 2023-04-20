use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::Context;
use bitcoin::hash_types::Txid;
use lightning::chain::transaction::OutPoint;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct LxOutPoint {
    pub txid: Txid,
    pub index: u16,
}

impl From<OutPoint> for LxOutPoint {
    fn from(op: OutPoint) -> Self {
        Self {
            txid: op.txid,
            index: op.index,
        }
    }
}

impl From<LxOutPoint> for OutPoint {
    fn from(op: LxOutPoint) -> Self {
        Self {
            txid: op.txid,
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

        let txid = Txid::from_str(txid_str)
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
