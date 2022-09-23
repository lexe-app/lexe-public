//! The `lexe-ln` crate contains shared Lexe newtypes for bitcoin / lightning
//! types (usually) defined in LDK.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

/// Type aliases.
pub mod alias;
/// BitcoinD client.
pub mod bitcoind;
/// Channel monitor
pub mod channel_monitor;
/// Helper *functions* used during init, typically to spawn tasks.
pub mod init;
/// Keys manager
pub mod keys_manager;
/// LDK + SGX compatible logger
pub mod logger;
/// Types related to peers and networking.
pub mod peer;
