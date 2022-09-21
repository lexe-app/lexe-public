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
/// Keys manager
pub mod keys_manager;
/// LDK + SGX compatible logger
pub mod logger;
