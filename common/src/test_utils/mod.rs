use std::net::{SocketAddr, TcpListener, TcpStream};

use once_cell::sync::Lazy;

use crate::api::ports::Port;
use crate::cli::LspInfo;
use crate::rng::WeakRng;
use crate::root_seed::RootSeed;

/// `Arbitrary`-like proptest strategies for foreign types.
pub mod arbitrary;
/// Quickly initialize a bitcoind regtest instance.
#[cfg(not(target_env = "sgx"))]
pub mod regtest;
/// Quickly create roundtrip proptest for various serialization schemes.
pub mod roundtrip;

// Dummy values for some commonly appearing fields
pub const DUMMY_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DUMMY_GATEWAY_URL: &str = "http://127.0.0.1:4040";
pub const DUMMY_RUNNER_URL: &str = "http://127.0.0.1:5050";
pub const DUMMY_LSP_URL: &str = "http://127.0.0.1:6060";
pub const DUMMY_ESPLORA_URL: &str = "http://127.0.0.1:7070";
pub static DUMMY_LSP_INFO: Lazy<LspInfo> = Lazy::new(|| {
    let mut rng = WeakRng::from_u64(20230216);
    let node_pk = RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng);
    let addr = SocketAddr::from(([127, 0, 0, 1], 42069));

    LspInfo {
        url: Some(DUMMY_LSP_URL.to_owned()),
        node_pk,
        addr,
        base_msat: 0,
        proportional_millionths: 3000,
        cltv_expiry_delta: 72,
        htlc_minimum_msat: 1,
        htlc_maximum_msat: u64::MAX,
    }
});

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
