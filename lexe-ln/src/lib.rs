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

use std::{fmt, future::Future, pin::Pin};

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
/// Bitcoin and Lightning-specific constants
pub mod constants;
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
/// Routing-related logic.
pub mod route;
/// Chain sync.
pub mod sync;
/// `TestEvent` channels and utils.
pub mod test_event;
/// Traits.
pub mod traits;
/// A transaction broadcaster task.
pub mod tx_broadcaster;
/// BDK wallet.
pub mod wallet;

/// The type we usually need for passing futures around.
pub type BoxedAnyhowFuture =
    Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Displays a slice of elements using each element's [`fmt::Display`] impl.
pub struct DisplaySlice<'a, T>(pub &'a [T]);

impl<T: fmt::Display> fmt::Display for DisplaySlice<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        write!(f, "[")?;
        for item in self.0 {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{item}")?;
        }
        write!(f, "]")
    }
}
