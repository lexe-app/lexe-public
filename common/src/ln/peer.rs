use std::fmt::{self, Display};
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::{bail, Context};
#[cfg(any(test, feature = "test-utils"))]
use once_cell::sync::Lazy;

use crate::api::NodePk;

/// A dummy [`ChannelPeer`] pointing to a non-existent LSP which can be passed
/// into the node during tests.
#[cfg(any(test, feature = "test-utils"))]
pub static DUMMY_LSP: Lazy<ChannelPeer> = Lazy::new(|| {
    use crate::rng::SmallRng;
    use crate::root_seed::RootSeed;

    let mut rng = SmallRng::new();
    let node_pk = RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng);
    let addr = SocketAddr::from(([127, 0, 0, 1], 42069));

    ChannelPeer { node_pk, addr }
});

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ChannelPeer {
    pub node_pk: NodePk,
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

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Strategy};
    use proptest::{prop_assert_eq, proptest};

    use super::*;
    use crate::rng::SmallRng;
    use crate::root_seed::RootSeed;
    use crate::test_utils::roundtrip;

    impl Arbitrary for ChannelPeer {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<SmallRng>(), any::<SocketAddr>())
                .prop_map(|(mut rng, mut addr)| {
                    let root_seed = RootSeed::from_rng(&mut rng);
                    let node_pk = root_seed.derive_node_pk(&mut rng);

                    // Hack to prevent the IPv6 flowinfo field (which we don't
                    // care about) from causing the equality check to fail
                    if let SocketAddr::V6(inner) = &mut addr {
                        inner.set_flowinfo(0);
                    }

                    Self { node_pk, addr }
                })
                .boxed()
        }
    }

    #[test]
    fn channel_peer_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<ChannelPeer>();
    }

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
    }
}
