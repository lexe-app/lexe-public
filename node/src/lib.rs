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

/// Type aliases.
mod alias;
/// Lexe API.
mod api;
/// Version approval and revocation.
mod approved_versions;
/// `NodeChannelManager` and related configs.
mod channel_manager;
/// `NodeEventHandler`.
mod event_handler;
/// `InactivityTimer`.
mod inactivity_timer;
/// Node-specific p2p logic
mod p2p;
/// `NodePeerManager`.
mod peer_manager;
/// `NodePersister` and related utils.
mod persister;
/// Node provision server and run body.
mod provision;
/// Node's main run body.
mod run;
/// Node's API server used while running.
mod server;
