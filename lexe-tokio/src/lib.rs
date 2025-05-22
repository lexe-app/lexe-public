//! A crate containing utilities and extensions built on top of Tokio.

/// Wraps a mpmc [`tokio::sync::broadcast`] to provide a convenient events bus.
pub mod events_bus;
/// A channel for sending deduplicated notifications with no data attached.
pub mod notify;
/// `NotifyOnce`, typically used as a shutdown channel.
pub mod notify_once;
/// `LxTask` and associated helpers.
pub mod task;

// Can save a `tokio` dependency declaration
pub use tokio;

// Default sizes for Tokio channels
pub const DEFAULT_CHANNEL_SIZE: usize = 256;
pub const SMALLER_CHANNEL_SIZE: usize = 16;
