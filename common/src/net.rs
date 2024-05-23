use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

/// A IPv6 [`SocketAddr`] to bind to localhost with an OS-assigned port.
// We should always try to use IPv6 because it will work everywhere;
// IPv4 may produce errors in some environments.
pub const LOCALHOST_WITH_EPHEMERAL_PORT: SocketAddr =
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0));

/// Returns an ephemeral port assigned by the OS which should be available for
/// the next ~60s after this function is called.
#[cfg(any(test, feature = "test-utils"))]
pub fn get_ephemeral_port() -> anyhow::Result<u16> {
    use std::net::{TcpListener, TcpStream};

    use anyhow::Context;

    // Request a random available port from the OS
    let listener = TcpListener::bind(LOCALHOST_WITH_EPHEMERAL_PORT)
        .expect("Could not bind TcpListener");
    let addr = listener
        .local_addr()
        .context("Could not get local address")?;

    // Create and accept a connection (which we'll promptly drop) in order to
    // force the port into the TIME_WAIT state, ensuring that the port will be
    // reserved from some limited amount of time (~60s on some Linux systems)
    let _sender =
        TcpStream::connect(addr).context("TcpStream::connect failed")?;
    let _incoming = listener.accept().context("TcpListener::accept failed")?;

    Ok(addr.port())
}
