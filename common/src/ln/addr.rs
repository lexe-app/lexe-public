use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    str::FromStr,
};

use anyhow::{Context, ensure, format_err};
use lightning::{ln::msgs::SocketAddress, util::ser::Hostname};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;

/// `LxSocketAddress` represents an internet address of a remote lightning
/// network peer.
///
/// It's morally equivalent to [`lightning::ln::msgs::SocketAddress`], but
/// intentionally ignores all TOR-related addresses since we don't currently
/// support TOR. It also has a well-defined human-readable serialization format,
/// unlike the LDK type.
#[derive(Clone, Eq, PartialEq, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxSocketAddress {
    TcpIpv4 {
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_ipv4_addr()")
        )]
        ip: Ipv4Addr,
        port: u16,
    },

    TcpIpv6 {
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

// There's no DNS resolution in SGX, so we can only impl for non SGX.
// When building for SGX, `TcpStream::connect` takes a string,
// and hostname resolution is done outside of the enclave.
#[cfg(not(target_env = "sgx"))]
impl std::net::ToSocketAddrs for LxSocketAddress {
    type Iter = std::vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        match self {
            LxSocketAddress::TcpIpv4 { ip, port } => {
                let addr = SocketAddr::V4(SocketAddrV4::new(*ip, *port));
                Ok(vec![addr].into_iter())
            }
            LxSocketAddress::TcpIpv6 { ip, port } => {
                let addr = SocketAddr::V6(SocketAddrV6::new(*ip, *port, 0, 0));
                Ok(vec![addr].into_iter())
            }
            // This branch does hostname resolution
            LxSocketAddress::TcpDns { hostname, port } =>
                (hostname.as_str(), *port).to_socket_addrs(),
        }
    }
}

impl From<SocketAddrV4> for LxSocketAddress {
    fn from(value: SocketAddrV4) -> Self {
        Self::TcpIpv4 {
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
        Ok(Self::TcpIpv6 {
            ip: *addr.ip(),
            port: addr.port(),
        })
    }
}

impl TryFrom<SocketAddr> for LxSocketAddress {
    type Error = anyhow::Error;
    fn try_from(value: SocketAddr) -> Result<Self, Self::Error> {
        match value {
            SocketAddr::V4(v4) => Ok(Self::from(v4)),
            SocketAddr::V6(v6) => Self::try_from(v6),
        }
    }
}

impl From<LxSocketAddress> for SocketAddress {
    fn from(value: LxSocketAddress) -> Self {
        match value {
            LxSocketAddress::TcpIpv4 { ip, port } => Self::TcpIpV4 {
                addr: ip.octets(),
                port,
            },
            LxSocketAddress::TcpIpv6 { ip, port } => Self::TcpIpV6 {
                addr: ip.octets(),
                port,
            },
            LxSocketAddress::TcpDns { hostname, port } =>
                Self::Hostname { hostname, port },
        }
    }
}

impl TryFrom<SocketAddress> for LxSocketAddress {
    type Error = anyhow::Error;
    fn try_from(value: SocketAddress) -> Result<Self, Self::Error> {
        match value {
            SocketAddress::TcpIpV4 { addr, port } => Ok(Self::TcpIpv4 {
                ip: Ipv4Addr::from(addr),
                port,
            }),
            SocketAddress::TcpIpV6 { addr, port } => Ok(Self::TcpIpv6 {
                ip: Ipv6Addr::from(addr),
                port,
            }),
            SocketAddress::Hostname { hostname, port } =>
                Ok(Self::TcpDns { hostname, port }),
            SocketAddress::OnionV2(..) | SocketAddress::OnionV3 { .. } =>
                Err(format_err!("TOR onion addresses are unsupported")),
        }
    }
}

// `<ip4 | ip6 | hostname>:<port>`
impl FromStr for LxSocketAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ensure!(!s.is_empty(), "empty string is invalid");

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

        ensure!(!prefix.is_empty(), "hostname can't be empty");

        // Try parsing as an IPv4 address.
        if let Ok(ip4) = Ipv4Addr::from_str(prefix) {
            return Ok(LxSocketAddress::TcpIpv4 { ip: ip4, port });
        }

        // Try parsing as a hostname / dns name.
        if let Ok(hostname) = Hostname::try_from(prefix.to_owned()) {
            return Ok(Self::TcpDns { hostname, port });
        }

        Err(format_err!("not a valid hostname or IP address"))
    }
}

impl fmt::Display for LxSocketAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TcpIpv4 { ip, port } =>
                fmt::Display::fmt(&SocketAddrV4::new(*ip, *port), f),
            Self::TcpIpv6 { ip, port } =>
                fmt::Display::fmt(&SocketAddrV6::new(*ip, *port, 0, 0), f),
            Self::TcpDns { hostname, port } =>
                write!(f, "{}:{port}", hostname.as_str()),
        }
    }
}

impl fmt::Debug for LxSocketAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn test_fromstr_json_equiv() {
        roundtrip::fromstr_json_string_equiv::<LxSocketAddress>();
    }

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
            LxSocketAddress::TcpIpv4 {
                ip: [1, 2, 3, 4].into(),
                port: 5050
            },
        );
        assert_eq!(
            LxSocketAddress::from_str("[::1]:5050").unwrap(),
            LxSocketAddress::TcpIpv6 {
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
}
