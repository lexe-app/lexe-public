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
