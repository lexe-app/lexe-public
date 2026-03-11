//! Lexe Rust SDK.

// NOTE: Docs for all stable APIs (i.e. all public items accessible via this
// crate) must be written for consumption by external users.
//
// Preview the public API rustdoc: `$ just docs-build-rustdoc --open`
//
// - Internal-facing documentation can still be written for public items, but
//   should be placed in `//` comments, not `///` comments, to avoid being
//   rendered in the public API docs at <rust.lexe.tech>.
// - `///` comments are still preferred for private, crate-private, and unstable
//   items.

#![deny(missing_docs)]

// --- Public API --- //
//
// All APIs accessible via these public modules must respect semver guarantees.

/// Configuration options for a `LexeWallet`.
pub mod config;
/// Types used by the Lexe SDK.
pub mod types;
/// `LexeWallet`: the top-level handle to a Lexe wallet.
pub mod wallet;

/// Returns the default Lexe data directory (`~/.lexe`).
pub use common::default_lexe_data_dir;

/// `BlockingLexeWallet`: synchronous wrapper around `LexeWallet`.
///
/// Enabled by the `blocking` feature flag.
#[cfg(feature = "blocking")]
pub mod blocking_wallet;

// Reexport possibly-useful dependencies
// TODO(max): Consider also re-exporting: dotenvy, serde
pub use anyhow;
pub use serde_json;
pub use tracing;

/// Initialize the Lexe logger with the given default log level.
///
/// Example: `lexe::init_logger("info")`
pub fn init_logger(default_level: &str) {
    lexe_logger::init_with_default(default_level);
}

// --- Unstable APIs --- //

/// This module ensures all unstable APIs are accessible within the crate, but
/// not to external users of the crate, unless they enable the `unstable`
/// feature, in which case they can access it via the re-export below.
mod unstable {
    /// A flat file system abstraction.
    pub mod ffs;
    /// Local payments database synced from the user node.
    pub mod payments_db;
    /// Provision-related utilities.
    pub mod provision;
    /// Wallet database.
    pub mod wallet_db;

    /// The user agent string used for SDK requests to Lexe infrastructure.
    ///
    /// Format: `lexe/<sdk_version> node/<latest_node_version>`
    ///
    /// Example: `lexe/0.1.0 node/0.8.11`
    pub static SDK_USER_AGENT: std::sync::LazyLock<&'static str> =
        std::sync::LazyLock::new(|| {
            // Get the latest node version.
            let releases = provision::releases_json();
            let node_releases =
                releases.0.get("node").expect("No 'node' in releases.json");
            let (latest_node_version, _release) =
                node_releases.last_key_value().expect("No node releases");

            let sdk_with_version = lexe_api::user_agent_to_lexe!();
            let user_agent =
                format!("{sdk_with_version} node/{latest_node_version}");

            Box::leak(user_agent.into_boxed_str())
        });
}

/// Opt-in to unstable APIs.
#[cfg(feature = "unstable")]
pub use unstable::*;
