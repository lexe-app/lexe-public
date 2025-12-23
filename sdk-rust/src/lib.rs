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

/// Reexported types needed by SDK consumers.
/// All types exported here are considered part of the stable public API.
pub mod types {
    pub use common::{api::user::UserPk, rng::SysRng, root_seed::RootSeed};
    pub use lexe_api::{
        models::command::UpdatePaymentNote,
        types::payments::{
            BasicPaymentV2, PaymentCreatedIndex, PaymentUpdatedIndex,
        },
    };
    pub use node_client::credentials::{
        ClientCredentials, Credentials, CredentialsRef,
    };
    pub use sdk_core::{
        models::{
            SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
            SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfo,
            SdkPayInvoiceRequest, SdkPayInvoiceResponse,
        },
        types::SdkPayment,
    };
}

// Reexport possibly-useful dependencies
pub use anyhow;
pub use serde_json;
pub use tracing;

/// Initialize the Lexe logger with the given default log level.
///
/// Example: `lexe_sdk::init_logger("info")`
pub fn init_logger(default_level: &str) {
    logger::init_with_default(default_level);
}

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
