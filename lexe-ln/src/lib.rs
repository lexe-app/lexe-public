//! The `lexe-ln` crate contains shared Lexe newtypes for bitcoin / lightning
//! types (usually) defined in LDK.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

/// Type aliases.
pub mod alias;
pub mod background_processor;
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
/// Chain sync.
pub mod sync;
/// Traits.
pub mod traits;
/// Misc types that temporarily don't fit anywhere else
pub mod types;
