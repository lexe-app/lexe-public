//! low-level SGX enclave types, constants, and platform functions.
//!
//! This crate tries to be dependency minimized, so other crates can
//! use types like `Measurement` with also pulling in a huge number of other
//! heavy dependencies.

// Re-export in this `enclave` module for nicer namespacing, e.g.,
// `enclave::measurement()` vs just `measurement()`.
pub mod enclave {
    pub use crate::{platform::*, types::*};
}

/// SGX platform functions, e.g., `measurement()`, `machine_id()`, ...
pub(crate) mod platform;
/// SGX enclave types, e.g., `Measurement`, `MachineId`, ...
pub(crate) mod types;
