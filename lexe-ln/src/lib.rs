//! The `lexe-ln` crate contains shared Lexe newtypes for bitcoin / lightning
//! types (usually) defined in LDK.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]

/// Type aliases.
pub mod alias;
pub mod background_processor;
/// BitcoinD client.
pub mod bitcoind;
/// Channel monitor
pub mod channel_monitor;
/// Keys manager
pub mod keys_manager;
/// LDK + SGX compatible logger
pub mod logger;
/// Shared functionality relating to LN P2P.
pub mod p2p;
/// Chain sync.
pub mod sync;
/// Traits.
pub mod traits;
/// Misc types that temporarily don't fit anywhere else
pub mod types;
