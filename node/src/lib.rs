//! Managed Lightning Network node that runs in a secure enclave.

use lexe_enclave::enclave;

pub mod cli;

/// Type aliases.
mod alias;
/// Logic to anonymize payment paths before reporting to Lexe LSP.
mod anonymize_path;
/// Version approval and revocation.
mod approved_versions;
/// Asynchronous backup worker.
mod backup_persister;
/// `NodeChannelManager` and related configs.
mod channel_manager;
/// API clients.
mod client;
/// Context shared between usernodess or initialized per usernode.
mod context;
/// `NodeEventHandler`.
mod event_handler;
/// GDrive backup writes.
mod gdrive_persister;
/// GDrive-specific setup logic.
mod gdrive_setup;
/// Meganode run body.
mod mega;
/// NWC specific logic.
mod nwc;
/// Node-specific p2p logic
mod p2p;
/// `NodePeerManager`.
mod peer_manager;
/// `NodePersister` and related utils.
mod persister;
/// Provision server and run body.
mod provision;
/// Run a single user node.
mod run;
/// `UserRunner`.
mod runner;
/// Node's API server used while running.
mod server;
/// Caches whether users exist.
mod user_cache;
/// VSS backup persister.
mod vss_persister;

/// Return the node semver version.
///
/// Normally this comes from the crate Cargo.toml version baked in at compile
/// time. For smoketest, `sgx-builder` patches in a `0.0.0-dev.1` or
/// `0.0.0-dev.2` version override so we can test re-provisioning multiple
/// nearly-identitcal versions with different measurements.
///
/// The version patching is done so we only need to build and link the enclaves
/// once.
///
/// See: [`enclave::dev`]
pub(crate) fn version() -> semver::Version {
    let version = enclave::dev::version().unwrap_or(env!("CARGO_PKG_VERSION"));
    semver::Version::parse(version).expect("Invalid node version")
}
