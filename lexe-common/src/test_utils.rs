/// `Arbitrary`-like proptest strategies for foreign types.
pub mod arbitrary;
/// Quickly create roundtrip proptest for various serialization schemes.
pub mod roundtrip;
/// Extremely basic snapshot testing
pub mod snapshot;

// Dummy values for some commonly appearing fields
pub const DUMMY_BACKEND_URL: &str = "http://[::1]:3030";
pub const DUMMY_GATEWAY_URL: &str = "http://[::1]:4040";
pub const DUMMY_RUNNER_URL: &str = "http://[::1]:5050";
pub const DUMMY_LSP_URL: &str = "http://[::1]:6060";
pub const DUMMY_ESPLORA_URL: &str = "http://[::1]:7070";
