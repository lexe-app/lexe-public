//! A crate containing utilities and extensions built on top of Tokio.

/// A channel for sending deduplicated notifications with no data attached.
pub mod notify;
/// `NotifyOnce`, typically used as a shutdown channel.
pub mod notify_once;
/// `LxTask` and associated helpers.
pub mod task;

// Can save a `tokio` dependency declaration
pub use tokio;
