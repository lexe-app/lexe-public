//! Lexe Rust SDK.

#![deny(missing_docs)]

// --- Public API --- //
//
// All APIs accessible via these public modules must respect semver guarantees.

/// Configuration options for a `LexeWallet`.
pub mod config;
/// `LexeWallet`: the top-level handle to a Lexe wallet.
pub mod wallet;

/// `BlockingLexeWallet`: synchronous wrapper around `LexeWallet`.
///
/// Enabled by the `blocking` feature flag.
#[cfg(feature = "blocking")]
pub mod blocking_wallet;

/// Returns the default Lexe data directory (`~/.lexe`).
pub use common::default_lexe_data_dir;

/// Reexported types needed by SDK consumers.
/// All types exported here are considered part of the stable public API.
pub mod types {
    pub use sdk_core::types::Order;

    /// Authentication, identity, and node verification.
    pub mod auth {
        pub use common::{
            api::user::{NodePk, UserPk},
            enclave::Measurement,
            root_seed::RootSeed,
        };
        pub use node_client::credentials::{
            ClientCredentials, Credentials, CredentialsRef,
        };
    }

    /// On-chain and Bitcoin primitives.
    pub mod bitcoin {
        pub use common::ln::{
            amount::Amount, hashes::LxTxid, priority::ConfirmationPriority,
        };
        pub use lexe_api::types::invoice::LxInvoice;
    }

    /// Request, response, and command types for SDK operations.
    pub mod command {
        pub use lexe_api::models::command::UpdatePaymentNote;
        pub use sdk_core::{
            models::{
                SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
                SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfo,
                SdkPayInvoiceRequest, SdkPayInvoiceResponse,
            },
            types::{ListPaymentsResponse, SdkPayment},
        };

        // TODO(max): PaymentSyncSummary should live in sdk-core. To
        // address the any_changes issue, we could probably just
        // delete the method, and inline its logic anywhere where
        // it's currently called (only one place).
        pub use crate::unstable::payments_db::PaymentSyncSummary;
    }

    /// Payment data and metadata.
    pub mod payment {
        pub use lexe_api::types::payments::{
            BasicPaymentV2, LxPaymentHash, LxPaymentId, LxPaymentSecret,
            PaymentCreatedIndex, PaymentDirection, PaymentKind, PaymentRail,
            PaymentStatus, PaymentUpdatedIndex,
        };
        pub use sdk_core::types::PaymentFilter;
    }

    /// General-purpose utilities.
    pub mod util {
        pub use common::time::TimestampMs;
    }
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
    /// Local payments database synced from the user node.
    pub mod payments_db;
    /// Provision-related utilities.
    pub mod provision;
    /// Wallet database.
    pub mod wallet_db;
}

/// Opt-in to unstable APIs.
#[cfg(feature = "unstable")]
pub use unstable::*;
