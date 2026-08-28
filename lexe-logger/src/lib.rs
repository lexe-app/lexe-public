//! Lexe tracing primitives and logger configuration.
//!
//! - The `trace` module holds `TraceId` and the statics which allow other
//!   crates (e.g. `lexe-api`) to read and write `TraceId`s in tracing span
//!   `Extensions` without knowing the concrete subscriber type.
//! - The `subscriber` feature enables the logger configuration for non-SGX lexe
//!   services, exposed at the crate root. See also: the `logger` module in the
//!   `public/lexe-ln` crate for log config in enclaves.

/// `TraceId` and the logger-side span plumbing for it.
pub mod trace;

/// `panic!(..)`s in debug mode, `tracing::error!(..)`s in release mode
#[macro_export]
macro_rules! debug_panic_release_log {
    ($($arg:tt)*) => {
        if core::cfg!(debug_assertions) {
            core::panic!($($arg)*);
        } else {
            tracing::error!($($arg)*);
        }
    };
}

#[cfg(feature = "subscriber")]
mod subscriber;
#[cfg(feature = "subscriber")]
pub use subscriber::*;
