use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

/// A IPv6 [`SocketAddr`] to bind to localhost with an OS-assigned port.
// We should always try to use IPv6 because it will work everywhere;
// IPv4 may produce errors in some environments.
pub const LOCALHOST_WITH_EPHEMERAL_PORT: SocketAddr =
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0));
