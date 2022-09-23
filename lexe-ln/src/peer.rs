use std::fmt::{self, Display};
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::Context;
use bitcoin::secp256k1::PublicKey;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelPeer {
    pub pk: PublicKey,
    pub addr: SocketAddr,
}

/// <pk>@<addr>
impl FromStr for ChannelPeer {
    type Err = anyhow::Error;
    fn from_str(pk_at_addr: &str) -> anyhow::Result<Self> {
        // vec![<pk>, <addr>]
        let mut pk_and_addr = pk_at_addr.split('@');
        let pk_str = pk_and_addr
            .next()
            .context("Missing <pk> in <pk>@<addr> peer address")?;
        let addr_str = pk_and_addr
            .next()
            .context("Missing <addr> in <pk>@<addr> peer address")?;

        let pk = PublicKey::from_str(pk_str)
            .context("Could not deserialize PublicKey from LowerHex")?;
        let addr = SocketAddr::from_str(addr_str)
            .context("Could not parse socket address from string")?;

        Ok(Self { pk, addr })
    }
}

/// <pk>@<addr>
impl Display for ChannelPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.pk, self.addr)
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use common::rng::SysRng;
    use common::root_seed::RootSeed;
    use proptest::arbitrary::any;
    use proptest::strategy::Strategy;
    use proptest::{prop_assert_eq, proptest};
    use secrecy::Secret;

    use super::*;

    proptest! {
        /// [`SocketAddr`] causes problems with proptest, this tests the fix.
        #[test]
        fn socket_addr_roundtrip(
            mut addr1 in any::<SocketAddr>(),
        ) {
            let mut addr2 = SocketAddr::from_str(&addr1.to_string()).unwrap();

            // Hack to prevent differing IPv6 flowinfo fields (which we don't
            // care about) from failing the equality comparison
            if let (SocketAddr::V6(inner1), SocketAddr::V6(inner2))=
                (&mut addr1, &mut addr2) {
                inner1.set_flowinfo(0);
                inner2.set_flowinfo(0);
            }

            prop_assert_eq!(addr1, addr2);
        }

        #[test]
        fn channel_peer_roundtrip(
            root_seed in any::<[u8; 32]>()
                .prop_map(Secret::new)
                .prop_map(RootSeed::new)
                .no_shrink()
            ,
            addr in any::<SocketAddr>(),
        ) {
            let mut rng = SysRng::new();
            let pk = root_seed.derive_node_pk(&mut rng);
            let mut channel_peer1 = ChannelPeer { pk, addr };
            let mut channel_peer2 =
                ChannelPeer::from_str(&channel_peer1.to_string()).unwrap();

            // Hack to prevent differing IPv6 flowinfo fields (which we don't
            // care about) from failing the equality comparison
            if let (SocketAddr::V6(inner1), SocketAddr::V6(inner2)) =
                (&mut channel_peer1.addr, &mut channel_peer2.addr) {
                inner1.set_flowinfo(0);
                inner2.set_flowinfo(0);
            }

            prop_assert_eq!(channel_peer1, channel_peer2);
        }
    }
}
