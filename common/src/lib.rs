//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Used in `hex` module. Not super necessary, but convenient.
#![feature(slice_as_chunks)]
// Used in `rng` module. Avoids a runtime panic.
#![feature(const_option)]
// Used in `enclave/sgx` module for sealing.
#![feature(split_array)]

pub mod attest;
pub mod client_node_certs;
pub mod ed25519;
pub mod enclave;
pub mod hex;
pub mod rng;
pub mod root_seed;
pub mod sha256;
