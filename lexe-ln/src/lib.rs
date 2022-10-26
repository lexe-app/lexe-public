//! The `lexe-ln` crate contains shared Lexe newtypes for bitcoin / lightning
//! types (usually) defined in LDK.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]
// Allow e.g. PS: Deref<Target: LexeInnerPersister> in generics
#![feature(associated_type_bounds)]

/// Type aliases.
pub mod alias;
pub mod background_processor;
/// BitcoinD client.
pub mod bitcoind;
/// Shared functionality relating to opening, closing, managing channels.
pub mod channel;
/// Channel monitor
pub mod channel_monitor;
/// Top level commands that can be initiated by the user.
pub mod command;
/// Event helpers.
pub mod event;
/// Types related to invoices.
pub mod invoice;
/// Keys manager
pub mod keys_manager;
/// LDK + SGX compatible logger
pub mod logger;
/// Shared functionality relating to LN P2P.
pub mod p2p;
/// Chain sync.
pub mod sync;
/// `TestEvent` channels and utils.
pub mod test_event;
/// Traits.
pub mod traits;
