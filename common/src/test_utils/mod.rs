/// Quickly initialize a bitcoind regtest instance.
#[cfg(not(target_env = "sgx"))]
pub mod regtest;
/// Quickly create roundtrip proptest for various serialization schemes.
pub mod roundtrip;

use std::net::{TcpListener, TcpStream};

use crate::api::ports::Port;

/// Returns an ephemeral port assigned by the OS which should be available for
/// the next ~60s after this function is called
pub fn get_ephemeral_port() -> Port {
    // Request a random available port from the OS
    let listener = TcpListener::bind(("localhost", 0))
        .expect("Could not bind TcpListener");
    let addr = listener.local_addr().unwrap();

    // Create and accept a connection (which we'll promptly drop) in order to
    // force the port into the TIME_WAIT state, ensuring that the port will be
    // reserved from some limited amount of time (~60s on some Linux systems)
    let _sender = TcpStream::connect(addr).unwrap();
    let _incoming = listener.accept().unwrap();

    addr.port()
}
