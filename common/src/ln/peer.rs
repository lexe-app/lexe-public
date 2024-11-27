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
    /// Whether this peer is outbound,
    /// i.e. we manually initiated a connection to it
    pub outbound: bool,
    /// - For outbound peers: Contains only the addresses that were *manually*
    ///   specified when we initiated a connection to this peer.
    /// - For inbound peers: This list is empty, since we lazily look up this
    ///   peer's addresses from the network graph as needed.
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
