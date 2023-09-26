//! Common logger configuration for non-SGX lexe services.
//!
//! See also: the `logger` module in the `public/lexe-ln` crate for log config
//! in enclaves.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

use std::str::FromStr;

use tracing::Level;
use tracing_subscriber::{
    filter::Targets,
    layer::{Layer, SubscriberExt},
    util::{SubscriberInitExt, TryInitError},
};

/// Initialize a global `tracing` logger.
///
/// + The logger will print enabled `tracing` events and spans to stdout.
/// + The default log level includes INFO, WARN, and ERROR events.
/// + You can change the log level or module filtering with an appropriate
///   `RUST_LOG` env var set. Read more about the syntax here:
///   <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html>
///
/// Panics if a logger is already initialized. This will fail if used in tests,
/// since multiple test threads will compete to set the global logger.
pub fn init() {
    try_init().expect("Failed to setup logger");
}

/// Use this to initialize the global logger in tests.
pub fn init_for_testing() {
    // Quickly skip logger setup if no env var set.
    if std::env::var_os("RUST_LOG").is_none() {
        return;
    }

    // Don't panic if there's already a logger setup. Multiple tests might try
    // setting the global logger.
    let _ = try_init();
}

/// Try to initialize a global logger. Will return an `Err` if there is another
/// global logger already set.
pub fn try_init() -> Result<(), TryInitError> {
    // TODO(phlip9): non-blocking writer for prod
    // see: https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html

    // Defaults to INFO logs if no `RUST_LOG` env var is set or we can't parse
    // the targets filter.
    let rust_log_filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|rust_log| Targets::from_str(&rust_log).ok())
        .unwrap_or_else(|| Targets::new().with_default(Level::INFO));

    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_level(true)
        .with_target(true)
        // Enable colored outputs for stdout.
        // TODO(max): This should be disabled when outputting to files - a
        //            second subscriber is probably needed.
        .with_ansi(true)
        .with_filter(rust_log_filter);

    tracing_subscriber::registry().with(stdout_log).try_init()
}
