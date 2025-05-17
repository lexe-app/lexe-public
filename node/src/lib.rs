//! Managed Lightning Network node that runs in a secure enclave.

/// The semver version as specified in the crate Cargo.toml, e.g. "0.1.0".
/// This is baked in at compile time and thus can be treated as a trusted input.
pub const SEMVER_VERSION: &str = env!("CARGO_PKG_VERSION");
lexe_std::const_assert!(!SEMVER_VERSION.is_empty());
/// A dev version specified via `DEV_VERSION` env at compile time.
/// This is "0.0.0-dev.1" or "0.0.0-dev.2" in dev; is [`None`] otherwise.
/// Exists so that we can create nearly-identical dev builds with different
/// measurements in order to test re-provisioning logic.
pub const DEV_VERSION: Option<&str> = option_env!("DEV_VERSION");

pub mod cli;

mod alias;
mod api;
mod approved_versions;
mod channel_manager;
mod event_handler;
mod inactivity_timer;
mod p2p;
mod peer_manager;
mod persister;
mod provision;
mod run;
mod server;
