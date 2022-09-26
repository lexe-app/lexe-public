/// Quickly initialize a bitcoind regtest instance.
#[cfg(not(target_env = "sgx"))]
pub mod bitcoind;
/// Quickly create roundtrip proptest for various serialization schemes.
pub mod roundtrip;
