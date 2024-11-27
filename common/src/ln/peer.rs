use std::fmt;

#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{api::user::NodePk, ln::addr::LxSocketAddress};

/// Represents a Lightning Network peer, which we might have a channel with.
#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct LnPeer {
    pub node_pk: NodePk,
    pub addrs: Vec<LxSocketAddress>,
}

/// If 1 address: `<node_pk>@<addr>`
/// If multiple (or zero) addresses: `<node_pk>@[<addr1>,<addr2>,...]`
impl fmt::Display for LnPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_pk = &self.node_pk;
        if self.addrs.len() == 1 {
            let addr = self.addrs.first().expect("Just checked non-empty");
            write!(f, "{node_pk}@{addr}")
        } else {
            let addrs = &self.addrs;
            write!(f, "{node_pk}@{addrs:?}")
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn test_json_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<LnPeer>();
    }
}
