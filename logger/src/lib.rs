//! Common logger configuration for non-SGX lexe services.
//!
//! See also: the `logger` module in the `public/lexe-ln` crate for log config
//! in enclaves.

use std::{env, io, str::FromStr};

use anyhow::anyhow;
#[cfg(doc)]
use lexe_api::trace::TraceId;
use lexe_api::{define_trace_id_fns, trace};
use tracing::{level_filters::LevelFilter, Level};
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
/// + The logger will print enabled `tracing` events and spans to stderr.
/// + You can change the log level or module filtering with an appropriate
///   `RUST_LOG` env var set. Read more about the syntax here:
///   <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html>
///
/// Panics if a logger is already initialized. This will fail if used in tests,
/// since multiple test threads will compete to set the global logger.
pub fn init() {
    try_init().expect("Failed to setup logger");
}

/// [`init`] but defaults to the given `RUST_LOG` value if not set in env.
pub fn init_with_default(rust_log_default: &str) {
    try_init_with_default(rust_log_default).expect("Failed to set up logger")
}

/// Use this to initialize the global logger in tests.
#[cfg(any(test, feature = "test-utils"))]
pub fn init_for_testing() {
    // Don't panic if there's already a logger setup.
    // Multiple tests might try setting the global logger.
    let _ = try_init();
}

/// Try to initialize a global logger.
/// Returns `Err` if another global logger is already set.
pub fn try_init() -> anyhow::Result<()> {
    // If `RUST_LOG` isn't set, use "off" to initialize a no-op subscriber so
    // that all the `TraceId` infrastructure still works somewhat normally.
    try_init_with_default("off")
}

/// [`try_init`] but defaults to the given `RUST_LOG` value if not set in env.
pub fn try_init_with_default(rust_log_default: &str) -> anyhow::Result<()> {
    let rust_log_env = env::var("RUST_LOG");
    let rust_log = rust_log_env.as_deref().unwrap_or(rust_log_default);

    subscriber(rust_log)
        .try_init()
        .context("Logger already initialized")?;

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
        FmtLayer<Registry, DefaultFields, Format<Compact>, fn() -> io::Stderr>,
        Targets,
        Registry,
    >,
    Registry,
>;

/// Generates our [`tracing::Subscriber`] impl by parsing a simplified target
/// filter from the passed in RUST_LOG value. We parse the targets list manually
/// because the `env_filter` brings in too many deps (like regex) for SGX.
/// Defaults to INFO logs if we can't parse the targets filter.
///
/// This function is extracted so that we can check the correctness of the
/// `SubscriberType` type alias, which allows us to downcast back to our
/// subscriber to recover `TraceId`s.
fn subscriber(rust_log: &str) -> SubscriberType {
    // TODO(phlip9): non-blocking writer for prod
    // see: https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html
    let targets = Targets::from_str(rust_log)
        .inspect_err(|e| eprintln!("Invalid RUST_LOG; using INFO: {e}"))
        .unwrap_or_else(|_| Targets::new().with_default(Level::INFO));

    let clamped_targets =
        if cfg!(any(test, debug_assertions, feature = "test-utils")) {
            // Allow TRACE logs in tests / debug builds.
            targets
        } else {
            // Disallow TRACE logs in production.
            enforce_log_policy(targets)
        };

    let stderr_log = tracing_subscriber::fmt::Layer::default()
        .compact()
        .with_level(true)
        .with_target(true)
        .with_writer(io::stderr as fn() -> io::Stderr)
        // Enable colored outputs.
        // TODO(max): This should be disabled when outputting to files - a
        //            second subscriber is probably needed.
        .with_ansi(true)
        .with_filter(clamped_targets);

    tracing_subscriber::registry().with(stderr_log)
}

/// Disallows TRACE logs as a default or for any specific target.
fn enforce_log_policy(targets: Targets) -> Targets {
    /// Sets a level to DEBUG if it is currently specified as TRACE.
    fn clamp_level(level: LevelFilter) -> LevelFilter {
        if level == LevelFilter::TRACE {
            LevelFilter::DEBUG
        } else {
            level
        }
    }

    // Disallow TRACE. Set the default level to INFO if no default is set.
    let clamped_default = match targets.default_level() {
        Some(level) => clamp_level(level),
        None => LevelFilter::INFO,
    };

    let targets = targets
        .into_iter()
        .map(|(target, level)| (target, clamp_level(level)))
        .collect::<Targets>();

    targets.with_default(clamped_default)
}

#[cfg(test)]
mod test {
    use std::env;

    use lexe_api::trace::TraceId;

    use super::*;

    #[test]
    fn get_and_insert_trace_ids() {
        // The test won't work properly if RUST_LOG is empty or "off".
        match env::var("RUST_LOG").ok() {
            Some(v) if v.starts_with("off") => return,
            Some(_) => (),
            None => return,
        }

        init_for_testing();
        TraceId::get_and_insert_test_impl();
    }
}
