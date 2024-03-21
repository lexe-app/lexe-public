//! Common logger configuration for non-SGX lexe services.
//!
//! See also: the `logger` module in the `public/lexe-ln` crate for log config
//! in enclaves.

use std::str::FromStr;

use anyhow::anyhow;
#[cfg(doc)]
use common::api::trace::TraceId;
use common::{api::trace, define_trace_id_fns};
use tracing::Level;
use tracing_subscriber::{
    filter::{Filtered, Targets},
    fmt::{
        format::{Compact, DefaultFields, Format},
        Layer as FmtLayer,
    },
    layer::{Layer as LayerTrait, Layered, SubscriberExt},
    util::SubscriberInitExt,
    Registry,
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
pub fn try_init() -> anyhow::Result<()> {
    subscriber().try_init().context("Logger already set")?;

    define_trace_id_fns!(SubscriberType);
    trace::GET_TRACE_ID_FN
        .set(get_trace_id_from_span)
        .map_err(|_| anyhow!("GET_TRACE_ID_FN already set"))?;
    trace::INSERT_TRACE_ID_FN
        .set(insert_trace_id_into_span)
        .map_err(|_| anyhow!("INSERT_TRACE_ID_FN already set"))?;

    Ok(())
}

/// The full type of our subscriber which is downcasted to when recovering
/// [`TraceId`]s. If having trouble naming this correctly, change this to some
/// dummy value (e.g. `u32`) and the compiler will tell you what it should be.
type SubscriberType = Layered<
    Filtered<
        FmtLayer<Registry, DefaultFields, Format<Compact>>,
        Targets,
        Registry,
    >,
    Registry,
>;

/// Generates our [`tracing::Subscriber`] impl. This function is extracted so
/// that we can check the correctness of the `SubscriberType` type alias, which
/// allows us to downcast back to our subscriber to recover [`TraceId`]s.
fn subscriber() -> SubscriberType {
    // TODO(phlip9): non-blocking writer for prod
    // see: https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html

    // Defaults to INFO logs if no `RUST_LOG` env var is set or we can't
    // parse the targets filter.
    let rust_log_filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|rust_log| Targets::from_str(&rust_log).ok())
        .unwrap_or_else(|| Targets::new().with_default(Level::INFO));

    let stdout_log = tracing_subscriber::fmt::layer()
        .compact()
        .with_level(true)
        .with_target(true)
        // Enable colored outputs for stdout.
        // NOTE: This should be disabled if outputting to files
        .with_ansi(true)
        .with_filter(rust_log_filter);

    tracing_subscriber::registry().with(stdout_log)
}

#[cfg(test)]
mod test {
    use common::api::trace::TraceId;

    use super::*;

    #[test]
    fn get_and_insert_trace_ids() {
        let _ = try_init();
        TraceId::get_and_insert_test_impl();
    }
}
