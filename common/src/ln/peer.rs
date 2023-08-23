use std::{
    fmt::{self, Display},
    net::SocketAddr,
    str::FromStr,
};

use anyhow::{bail, Context};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::api::NodePk;
#[cfg(test)]
use crate::test_utils::arbitrary;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct ChannelPeer {
    pub node_pk: NodePk,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_socket_addr()"))]
    pub addr: SocketAddr,
}

/// `<node_pk>@<addr>`
impl FromStr for ChannelPeer {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        // vec![<node_pk>, <addr>]
        let mut parts = s.split('@');
        let (pk_str, addr_str) =
            match (parts.next(), parts.next(), parts.next()) {
                (Some(pk_str), Some(addr_str), None) => (pk_str, addr_str),
                _ => bail!("Should be in format <node_pk>@<socket_addr>"),
            };

        let node_pk =
            NodePk::from_str(pk_str).context("Invalid node public key")?;
        let addr =
            SocketAddr::from_str(addr_str).context("Invalid socket address")?;

        Ok(Self { node_pk, addr })
    }
}

/// `<node_pk>@<addr>`
impl Display for ChannelPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_pk = &self.node_pk;
        let addr = &self.addr;
        write!(f, "{node_pk}@{addr}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn channel_peer_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<ChannelPeer>();
    }
}
