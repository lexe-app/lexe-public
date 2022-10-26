use std::error::Error;
use std::ops::Deref;
use std::str::FromStr;

use lightning::util::logger::{Level as LdkLevel, Logger, Record};
use once_cell::sync::Lazy;
use tracing_core::field::{Field, FieldSet, Value};
use tracing_core::subscriber::Interest;
use tracing_core::{
    dispatcher, identify_callsite, Callsite, Event, Kind, Level, Metadata,
};

pub type TracingError = Box<dyn Error + Send + Sync + 'static>;

pub fn init() {
    try_init().expect("Failed to setup logger");
}

/// Try to initialize a global logger. Will return an `Err` if there is another
/// global logger already set.
///
/// The log level is configurable via the `RUST_LOG` environment variable. For
/// example, `RUST_LOG=trace cargo run` would run the node with all logs
/// enabled.
pub fn try_init() -> Result<(), TracingError> {
    // For the node, just parse a blanket level from the env. The `env_filter`
    // feature pulls in too many dependencies for this use-case IMO.
    //
    // Defaults to INFO logs if no `RUST_LOG` env var is set.
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|rust_log| Level::from_str(&rust_log).ok())
        .unwrap_or(Level::INFO);

    tracing_subscriber::fmt()
        .compact()
        .with_level(true)
        // Enable colored outputs for stdout. TODO(max): This should be disabled
        // when outputting to files - a second subscriber is probably needed.
        .with_ansi(true)
        .with_max_level(level)
        .try_init()
}

pub fn init_for_testing() {
    // Quickly skip logger setup if no env var set.
    if std::env::var_os("RUST_LOG").is_none() {
        return;
    }

    // Don't panic if there's already a logger setup. Multiple tests might try
    // setting the global logger.
    let _ = try_init();
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
    fn log(&self, record: &Record) {
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
                    (&keys.file, Some(&record.file)),
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
    file: Field,
    line: Field,
}

static FIELD_NAMES: &[&str] = &["message", "module", "file", "line"];

impl Fields {
    fn new(cs: &'static dyn Callsite) -> Self {
        let fieldset = cs.metadata().fields();
        let message = fieldset.field("message").unwrap();
        let module = fieldset.field("module").unwrap();
        let file = fieldset.field("file").unwrap();
        let line = fieldset.field("line").unwrap();
        Fields {
            message,
            module,
            file,
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

static TRACE_FIELDS: Lazy<Fields> = Lazy::new(|| Fields::new(&TRACE_CS));
static DEBUG_FIELDS: Lazy<Fields> = Lazy::new(|| Fields::new(&DEBUG_CS));
static INFO_FIELDS: Lazy<Fields> = Lazy::new(|| Fields::new(&INFO_CS));
static WARN_FIELDS: Lazy<Fields> = Lazy::new(|| Fields::new(&WARN_CS));
static ERROR_FIELDS: Lazy<Fields> = Lazy::new(|| Fields::new(&ERROR_CS));

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
    use std::collections::HashMap;

    use lightning::{log_given_level, log_internal};
    use tracing_core::span::{Attributes, Id, Record};
    use tracing_core::{Dispatch, Subscriber};

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
                        self.0.insert(field.name(), format!("{:?}", value));
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
                assert!(fields.contains_key("file"));
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
            lightning::log_error!(ldk_logger, "hello: {}", 123);
        });
    }
}
