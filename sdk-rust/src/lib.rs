//! Lexe Rust SDK.

#![deny(missing_docs)]

// --- Public API --- //
//
// All APIs accessible via these public modules must respect semver guarantees.

/// Configuration options for a `LexeWallet`.
pub mod config;
/// Local payments database synced from the user node.
pub mod payments_db;
/// `LexeWallet`: the top-level handle to a Lexe wallet.
pub mod wallet;

// --- Unstable APIs --- //

/// This module ensures all unstable APIs are accessible within the crate, but
/// not to external users of the crate, unless they enable the `unstable`
/// feature, in which case they can access it via the re-export below.
mod unstable {
    /// `Ffs`: A flat file system abstraction.
    pub mod ffs;
    /// Provision-related utilities.
    pub mod provision;
    /// `ProvisionHistory`
    // TODO(max): Delete this module once we calculate `enclaves_to_provision`
    // in the backend, so provisioning can be stateless. Remember, however, that
    // we have to check that all `NodeEnclave`s the backend returns is inside
    // `LATEST_TRUSTED_MEASUREMENTS`.
    pub mod provision_history;
    /// Wallet database.
    pub mod wallet_db;
}

/// Opt-in to unstable APIs.
#[cfg(feature = "unstable")]
pub use unstable::*;
