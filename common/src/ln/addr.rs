use std::{
    fmt::{self, Display},
    net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6},
    str::FromStr,
};

use anyhow::{ensure, Context};
use lightning::util::ser::Hostname;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;

/// `LxSocketAddress` represents an internet address of a remote lightning
/// network peer.
///
/// It's morally equivalent to [`lightning::ln::msgs::NetAddress`] (named
/// `SocketAddress` on LDK master), but intentionally ignores all TOR-related
/// addresses since we don't currently support TOR. It also has a well-defined
/// human-readable serialization format, unlike the LDK type.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxSocketAddress {
    TcpIp4 {
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_ipv4_addr()")
        )]
        ip: Ipv4Addr,
        port: u16,
    },

    TcpIp6 {
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_ipv6_addr()")
        )]
        ip: Ipv6Addr,
        port: u16,
    },

    TcpDns {
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_hostname()")
        )]
        hostname: Hostname,
        port: u16,
    },
    // Intentionally left out: OnionV2, OnionV3
    // We don't support TOR connections atm.
}

impl From<SocketAddrV4> for LxSocketAddress {
    fn from(value: SocketAddrV4) -> Self {
        Self::TcpIp4 {
            ip: *value.ip(),
            port: value.port(),
        }
    }
}

impl TryFrom<SocketAddrV6> for LxSocketAddress {
    type Error = anyhow::Error;
    fn try_from(addr: SocketAddrV6) -> Result<Self, Self::Error> {
        ensure!(
            addr.scope_id() == 0 && addr.flowinfo() == 0,
            "IPv6 address' scope_id and flowinfo must both be zero"
        );
        Ok(Self::TcpIp6 {
            ip: *addr.ip(),
            port: addr.port(),
        })
    }
}

impl FromStr for LxSocketAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(anyhow::format_err!("empty string is invalid"));
        }

        let first_byte = s.as_bytes()[0];

        // IPv6 socket addr format always starts with '['.
        if first_byte == b'[' {
            // Reuse the SocketAddrV6 parser, but make sure we reject any inputs
            // with extra scope_id or flowinfo.
            let sockaddr6 = SocketAddrV6::from_str(s)
                .context("invalid IPv6 socket address")?;
            return Self::try_from(sockaddr6);
        }

        // Try to parse out the port.
        let (prefix, port_str) =
            s.rsplit_once(':').context("port is required")?;
        let port = u16::from_str(port_str).context("invalid port")?;

        if prefix.is_empty() {
            return Err(anyhow::format_err!("hostname can't be empty"));
        }

        // Try parsing as an IPv4 address.
        if let Ok(ip4) = Ipv4Addr::from_str(prefix) {
            return Ok(LxSocketAddress::TcpIp4 { ip: ip4, port });
        }

        // Try parsing as a hostname / dns name.
        if let Ok(hostname) = Hostname::try_from(prefix.to_owned()) {
            return Ok(Self::TcpDns { hostname, port });
        }

        Err(anyhow::format_err!("not a valid hostname or IP address"))
    }
}

impl Display for LxSocketAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TcpIp4 { ip, port } =>
                Display::fmt(&SocketAddrV4::new(*ip, *port), f),
            Self::TcpIp6 { ip, port } =>
                Display::fmt(&SocketAddrV6::new(*ip, *port, 0, 0), f),
            Self::TcpDns { hostname, port } =>
                write!(f, "{}:{port}", hostname.as_str()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn test_basic() {
        // bad
        LxSocketAddress::from_str("").unwrap_err();
        LxSocketAddress::from_str("foo").unwrap_err();
        LxSocketAddress::from_str("foo:").unwrap_err();
        LxSocketAddress::from_str("1.2.3.4:").unwrap_err();
        LxSocketAddress::from_str(":123").unwrap_err();
        LxSocketAddress::from_str("1.2.3.4:65538").unwrap_err();
        LxSocketAddress::from_str("[::1]:65538").unwrap_err();
        LxSocketAddress::from_str("[::1%6969]:5050").unwrap_err();
        LxSocketAddress::from_str("hello! world!:5050").unwrap_err();

        // good
        assert_eq!(
            LxSocketAddress::from_str("1.2.3.4:5050").unwrap(),
            LxSocketAddress::TcpIp4 {
                ip: [1, 2, 3, 4].into(),
                port: 5050
            },
        );
        assert_eq!(
            LxSocketAddress::from_str("[::1]:5050").unwrap(),
            LxSocketAddress::TcpIp6 {
                ip: [0_u16, 0, 0, 0, 0, 0, 0, 1].into(),
                port: 5050
            },
        );
        assert_eq!(
            LxSocketAddress::from_str("lsp.lexe.app:9735").unwrap(),
            LxSocketAddress::TcpDns {
                hostname: Hostname::try_from("lsp.lexe.app".to_owned())
                    .unwrap(),
                port: 9735
            },
        );
    }

    #[test]
    fn test_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxSocketAddress>();
    }
}
