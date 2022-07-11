//! The `common` crate contains types and functionality shared between the Lexe
//! node and client code.

// Used in `hex` module. Not super necessary, but convenient.
#![feature(slice_as_chunks)]

pub mod ed25519;
pub mod hex;
