//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Used in `hex` module. Not super necessary, but convenient.
#![feature(slice_as_chunks)]
// Used in `rng` module. Avoids a runtime panic.
#![feature(const_option)]
// Used in `enclave/sgx` module for sealing.
#![feature(split_array)]
// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

// re-export some common types from our dependencies
pub use bitcoin::secp256k1::PublicKey;
pub use secrecy::Secret;

pub mod api;
pub mod attest;
pub mod cli;
pub mod client;
pub mod constants;
pub mod ed25519;
pub mod enclave;
pub mod hex;
pub mod hexstr_or_bytes;
pub mod ln;
pub mod rng;
pub mod root_seed;
pub mod sha256;
