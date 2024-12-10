//! The `lexe-ln` crate contains shared Bitcoin + Lightning logic, utilities,
//! and newtypes.

// Ignore this issue with `proptest_derive::Arbitrary`.
#![allow(clippy::arc_with_non_send_sync)]
// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]
// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]
// Ignore this useless lint
#![allow(clippy::new_without_default)]

/// Type aliases.
pub mod alias;
/// Background processor.
pub mod background_processor;
/// Utilities for computing lightning balances.
pub mod balance;
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
pub mod payments;
/// Shared persister logic.
pub mod persister;
/// Chain sync.
pub mod sync;
/// `TestEvent` channels and utils.
pub mod test_event;
/// Traits.
pub mod traits;
/// BDK wallet.
pub mod wallet;
