//! The `lexe-ln` crate contains shared Bitcoin + Lightning logic, utilities,
//! and newtypes.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]
// Allow e.g. PS: Deref<Target: LexeInnerPersister> in generics
#![feature(associated_type_bounds)]
// Allow adding side effects to Option and Result chains
#![feature(result_option_inspect)]

/// Type aliases.
pub mod alias;
/// Background processor.
pub mod background_processor;
/// Shared functionality relating to opening, closing, managing channels.
pub mod channel;
/// Channel monitor
pub mod channel_monitor;
/// Top level commands that can be initiated by the user.
pub mod command;
/// Esplora client.
pub mod esplora;
/// Event helpers.
pub mod event;
/// Keys manager
pub mod keys_manager;
/// LDK + SGX compatible logger
pub mod logger;
/// Shared functionality relating to LN P2P.
pub mod p2p;
/// Payments types.
pub mod payment;
/// Chain sync.
pub mod sync;
/// `TestEvent` channels and utils.
pub mod test_event;
/// Traits.
pub mod traits;
/// BDK wallet.
pub mod wallet;
