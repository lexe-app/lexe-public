//! # logger
//!
//! This module contains the logging config for SGX nodes.
//!
//! During development, the log level is configurable via the `RUST_LOG`
//! environment variable. For example, `RUST_LOG=trace cargo run` would run the
//! node with all logs enabled.
//!
//! Recall that in production, SGX enclaves won't see environment variables.
//! In either case, the log level defaults to `RUST_LOG=info`.
//!
//! ### Per-Target Filtering
//!
//! You can also filter logs on a per-crate/per-module basis:
//!
//! ```bash
//! $ RUST_LOG=warn,node=debug,hyper::client::conn=error cargo run node
//! ```
//!
//! Breaking down the above example, it would:
//!
//! 1. expose all `DEBUG`+ logs from the `node` crate
//! 2. silence all logs except `ERROR`s from the `conn` module in `hyper`
//! 3. default all other targets to `WARN`+
//!
//! ### Syntax
//!
//! The full syntax is, `RUST_LOG=<filter_1>,<filter_2>,...,<filter_n>`,
//! where each `<filter_i>` is of the form:
//!
//! * `trace` (bare LEVEL)
//! * `foo` (bare TARGET)
//! * `foo=trace` (TARGET=LEVEL)
//! * `foo[{bar,baz}]=info` (TARGET[{FIELD,+}]=LEVEL)

use std::{io, ops::Deref, str::FromStr, sync::LazyLock};

use anyhow::anyhow;
use lexe_api::{define_trace_id_fns, trace};
use lightning::util::logger::{Level as LdkLevel, Logger, Record};
use tracing_core::{
    Callsite, Event, Kind, Level, LevelFilter, Metadata, dispatcher,
    field::{Field, FieldSet, Value},
    identify_callsite,
    subscriber::Interest,
};
use tracing_subscriber::{
    Layer as LayerTrait, Registry,
    filter::{Filtered, Targets},
    fmt::{
        Layer,
        format::{Compact, DefaultFields, Format},
    },
    layer::{Layered, SubscriberExt},
    util::SubscriberInitExt,
};

/// Initialize the global `tracing` logger.
///
/// + The logger will print enabled `tracing` events and spans to stderr.
/// + The default log level includes INFO, WARN, and ERROR events.
///
/// Panics if a logger is already initialized. This will fail if used in tests,
/// since multiple test threads will compete to set the global logger.
pub fn init(rust_log: Option<&str>, allow_trace: bool) {
    try_init(rust_log, allow_trace).expect("Failed to setup logger");
}

/// Use this to initialize the global logger in tests.
#[cfg(any(test, feature = "test-utils"))]
pub fn init_for_testing() {
    let rust_log = std::env::var("RUST_LOG").ok();

    // Don't panic if there's already a logger setup.
    // Multiple tests might try setting the global logger.
    let _ = try_init(rust_log.as_deref(), true);
}

/// Try to initialize a global logger.
/// Returns `Err` if another global logger is already set.
pub fn try_init(
    rust_log: Option<&str>,
    allow_trace: bool,
) -> anyhow::Result<()> {
    // If `RUST_LOG` isn't set, use "off" to initialize a no-op subscriber so
    // that all the `TraceId` infrastructure still works somewhat normally.
    let rust_log = rust_log.unwrap_or("off");

    subscriber(rust_log, allow_trace)
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
/// `TraceId`s. If having trouble naming this correctly, change this to some
/// dummy value (e.g. `u32`) and the compiler will tell you what it should be.
type SubscriberType = Layered<
    Filtered<
        Layer<Registry, DefaultFields, Format<Compact>, fn() -> io::Stderr>,
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
fn subscriber(rust_log: &str, allow_trace: bool) -> SubscriberType {
    // TODO(phlip9): non-blocking writer for prod
    // see: https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html
    let targets = Targets::from_str(rust_log)
        .inspect_err(|e| eprintln!("Invalid RUST_LOG; using INFO: {e}"))
        .unwrap_or_else(|_| Targets::new().with_default(Level::INFO));

    // Allow TRACE logs in debug builds or if explicitly requested
    let clamped_targets =
        if cfg!(any(test, debug_assertions, feature = "test-utils"))
            || allow_trace
        {
            targets
        } else {
            clamp_targets(targets)
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
fn clamp_targets(targets: Targets) -> Targets {
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

// -- LexeTracingLogger -- //

/// An adapter that impls LDK's [`Logger`] trait and dispatches LDK logs to the
/// current registered [`tracing`] log backend.
///
/// It is fine to clone and use the LexeTracingLogger directly.
///
/// [`Logger`]: lightning::util::logger::Logger
/// [`tracing`]: https://crates.io/crates/tracing
#[derive(Clone)]
pub struct LexeTracingLogger(InnerTracingLogger);

impl LexeTracingLogger {
    pub fn new() -> Self {
        Self(InnerTracingLogger)
    }
}

impl Deref for LexeTracingLogger {
    type Target = InnerTracingLogger;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for LexeTracingLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct InnerTracingLogger;

impl Logger for InnerTracingLogger {
    /// Convert LDK log records to [`tracing::Event`]s and then dispatch them
    /// to the current registered [`tracing::Subscriber`].
    fn log(&self, record: Record) {
        dispatcher::get_default(|dispatch| {
            // unfortunately, we can't just `tracing::event!()` here, since the
            // log-level isn't known at compile time (which tracing requires).
            //
            // instead, we make 5 different static `Callsite` instances each
            // with a corresponding log-level. this function takes the dynamic
            // record.level from LDK and maps it to a static `Callsite`.
            let (keys, meta) = loglevel_to_cs(record.level);

            // early exit if the subscriber is not interested.
            if !dispatch.enabled(meta) {
                return;
            }

            let current_span = tracing::Span::current();

            dispatch.event(&Event::new_child_of(
                current_span.id(),
                meta,
                &meta.fields().value_set(&[
                    (&keys.message, Some(&record.args as &dyn Value)),
                    (&keys.module, Some(&record.module_path)),
                    (&keys.line, Some(&record.line)),
                ]),
            ));
        });
    }
}

// This section is adapted from the [`tracing-log`] crate, which does this same
// sort of conversion but for the standard `log` crate.
//
// [`tracing-log`]: https://docs.rs/tracing-log

struct Fields {
    message: Field,
    module: Field,
    line: Field,
}

static FIELD_NAMES: &[&str] = &["message", "module", "line"];

impl Fields {
    fn new(cs: &'static dyn Callsite) -> Self {
        let fieldset = cs.metadata().fields();
        let message = fieldset.field("message").unwrap();
        let module = fieldset.field("module").unwrap();
        let line = fieldset.field("line").unwrap();
        Fields {
            message,
            module,
            line,
        }
    }
}

macro_rules! log_cs {
    ($level:expr, $cs:ident, $meta:ident, $ty:ident) => {
        struct $ty;
        static $cs: $ty = $ty;
        static $meta: Metadata<'static> = Metadata::new(
            "ldk log event",
            "ldk",
            $level,
            None,
            None,
            None,
            FieldSet::new(FIELD_NAMES, identify_callsite!(&$cs)),
            Kind::EVENT,
        );

        impl Callsite for $ty {
            fn set_interest(&self, _: Interest) {}
            fn metadata(&self) -> &'static Metadata<'static> {
                &$meta
            }
        }
    };
}

log_cs!(Level::TRACE, TRACE_CS, TRACE_META, TraceCallsite);
log_cs!(Level::DEBUG, DEBUG_CS, DEBUG_META, DebugCallsite);
log_cs!(Level::INFO, INFO_CS, INFO_META, InfoCallsite);
log_cs!(Level::WARN, WARN_CS, WARN_META, WarnCallsite);
log_cs!(Level::ERROR, ERROR_CS, ERROR_META, ErrorCallsite);

static TRACE_FIELDS: LazyLock<Fields> =
    LazyLock::new(|| Fields::new(&TRACE_CS));
static DEBUG_FIELDS: LazyLock<Fields> =
    LazyLock::new(|| Fields::new(&DEBUG_CS));
static INFO_FIELDS: LazyLock<Fields> = LazyLock::new(|| Fields::new(&INFO_CS));
static WARN_FIELDS: LazyLock<Fields> = LazyLock::new(|| Fields::new(&WARN_CS));
static ERROR_FIELDS: LazyLock<Fields> =
    LazyLock::new(|| Fields::new(&ERROR_CS));

fn loglevel_to_cs(
    level: LdkLevel,
) -> (&'static Fields, &'static Metadata<'static>) {
    match level {
        LdkLevel::Trace | LdkLevel::Gossip => (&*TRACE_FIELDS, &TRACE_META),
        LdkLevel::Debug => (&DEBUG_FIELDS, &DEBUG_META),
        LdkLevel::Info => (&INFO_FIELDS, &INFO_META),
        LdkLevel::Warn => (&WARN_FIELDS, &WARN_META),
        LdkLevel::Error => (&ERROR_FIELDS, &ERROR_META),
    }
}

// -- Tests -- //

#[cfg(test)]
mod test {
    use std::{collections::HashMap, env};

    use lexe_api::trace::TraceId;
    use tracing_core::{
        Dispatch, Subscriber,
        span::{Attributes, Id, Record},
    };

    use super::*;

    #[test]
    fn test_ldk_tracing_logger() {
        struct MockSubscriber;

        impl Subscriber for MockSubscriber {
            fn enabled(&self, _: &Metadata<'_>) -> bool {
                true
            }
            fn event(&self, event: &Event<'_>) {
                // make a hashmap name -> value of all the log fields
                struct HashMapVisitor(HashMap<&'static str, String>);

                impl tracing_core::field::Visit for HashMapVisitor {
                    fn record_debug(
                        &mut self,
                        field: &Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        self.0.insert(field.name(), format!("{value:?}"));
                    }
                }

                // should have the right level
                assert_eq!(event.metadata().level(), &Level::ERROR);

                // collect all the fields
                let mut visitor = HashMapVisitor(HashMap::new());
                event.record(&mut visitor);
                let fields = visitor.0;

                // should contain the expected message and fields
                assert_eq!(
                    fields.get("message"),
                    Some("hello: 123".to_owned()).as_ref()
                );
                assert!(fields.contains_key("module"));
                assert!(!fields.contains_key("file"));
                assert!(fields.contains_key("line"));
            }
            fn enter(&self, _: &Id) {}
            fn exit(&self, _: &Id) {}
            fn new_span(&self, _: &Attributes) -> Id {
                Id::from_u64(0xf00)
            }
            fn record(&self, _: &Id, _: &Record<'_>) {}
            fn record_follows_from(&self, _: &Id, _: &Id) {}
        }

        let dispatch = Dispatch::new(MockSubscriber);

        dispatcher::with_default(&dispatch, || {
            let ldk_logger = LexeTracingLogger::new();
            // LDK 0.0.125: `lightning::log_error!(ldk_logger, "hello: {}",
            // 123);` doesn't work as it expects LDK features to be
            // present in our own crate, which clippy now errors on.
            //
            // TODO(phlip9): revert when we update to latest LDK master.
            ldk_logger.log(lightning::util::logger::Record {
                level: LdkLevel::Error,
                peer_id: None,
                channel_id: None,
                args: format_args!("hello: {}", 123),
                module_path: "lexe_ln::logger::test",
                file: "logger.rs",
                line: 123,
                payment_hash: None,
            });
        });
    }

    #[test]
    fn get_and_insert_trace_ids() {
        // The test won't work properly if RUST_LOG is empty or "off".
        let rust_log = match env::var("RUST_LOG").ok() {
            Some(v) if v.starts_with("off") => return,
            Some(v) => v,
            None => return,
        };

        let allow_trace = false;
        let _ = try_init(Some(&rust_log), allow_trace);
        TraceId::get_and_insert_test_impl();
    }
}
