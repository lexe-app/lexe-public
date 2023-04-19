//! Pipe `tracing` log messages from native Rust to Dart.

#![allow(dead_code)]

use std::fmt::{self, Write};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flutter_rust_bridge::StreamSink;
use tracing::{field, span, Event, Level, Subscriber};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};

struct DartLogLayer {
    rust_log_tx: StreamSink<String>,
}

pub(crate) fn init(rust_log_tx: StreamSink<String>, rust_log: &str) {
    try_init(rust_log_tx, rust_log).expect("logger is already set!");
}

pub(crate) fn init_for_testing(
    rust_log_tx: StreamSink<String>,
    rust_log: &str,
) {
    // Quickly skip logger setup if no env var set.
    if std::env::var_os("RUST_LOG").is_none() {
        return;
    }

    // Don't panic if there's already a logger setup. Multiple tests might try
    // setting the global logger.
    let _ = try_init(rust_log_tx, rust_log);
}

pub(crate) fn try_init(
    rust_log_tx: StreamSink<String>,
    rust_log: &str,
) -> Result<(), TryInitError> {
    let rust_log_filter = Targets::from_str(rust_log)
        .ok()
        .unwrap_or_else(|| Targets::new().with_default(Level::INFO));

    let dart_log_layer = DartLogLayer::new(rust_log_tx);

    tracing_subscriber::registry()
        .with(dart_log_layer.with_filter(rust_log_filter))
        .try_init()
}

impl DartLogLayer {
    fn new(rust_log_tx: StreamSink<String>) -> Self {
        Self { rust_log_tx }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for DartLogLayer {
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut message = String::new();
        fmt_event(&mut message, event, ctx).expect("Failed to format");

        self.rust_log_tx.add(message);
    }
}

// Adapted from:
// [`Format::<Compact, T>`::format_event`](https://github.com/tokio-rs/tracing/blob/tracing-subscriber-0.3.16/tracing-subscriber/src/fmt/format/mod.rs#L1012)
fn fmt_event<S: Subscriber + for<'a> LookupSpan<'a>>(
    buf: &mut String,
    event: &Event<'_>,
    ctx: Context<'_, S>,
) -> fmt::Result {
    let meta = event.metadata();
    let level = meta.level().as_str();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64();

    // pad INFO and WARN so log messages align
    let level_pad = if level.len() == 4 { " " } else { "" };
    let target = meta.target();

    write!(buf, "{timestamp:.06} {level_pad}{level}")?;
    fmt_spans(buf, event.parent(), ctx)?;
    write!(buf, " {target}:")?;

    // event fields
    event.record(&mut FieldVisitor::new(buf));
    Ok(())
}

// Adapted from:
// [`DefaultVisitor`](https://github.com/tokio-rs/tracing/blob/tracing-subscriber-0.3.16/tracing-subscriber/src/fmt/format/mod.rs#L1222)
struct FieldVisitor<'a> {
    buf: &'a mut String,
}

impl<'a> FieldVisitor<'a> {
    fn new(buf: &'a mut String) -> Self {
        Self { buf }
    }
}

impl<'a> field::Visit for FieldVisitor<'a> {
    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "message" {
            self.record_debug(field, &format_args!("{}", value))
        } else {
            self.record_debug(field, &value)
        }
    }

    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => write!(self.buf, " {value:?}"),
            // skip `log` crate metadata
            name if name.starts_with("log.") => Ok(()),
            name => write!(self.buf, " {name}={value:?}"),
        }
        .expect("Failed to write??");
    }
}

// Adapted from:
// [`FmtCtx::fmt`](https://github.com/tokio-rs/tracing/blob/tracing-subscriber-0.3.16/tracing-subscriber/src/fmt/format/mod.rs#L1353)
fn fmt_spans<S: Subscriber + for<'a> LookupSpan<'a>>(
    buf: &mut String,
    span: Option<&span::Id>,
    ctx: Context<'_, S>,
) -> fmt::Result {
    let span = span
        .and_then(|id| ctx.span(id))
        .or_else(|| ctx.lookup_current());

    let scope = span.into_iter().flat_map(|span| span.scope().from_root());

    let mut first = true;
    for span in scope {
        if first {
            buf.write_char(' ')?;
            first = false;
        }
        write!(buf, "{}:", span.metadata().name())?;
    }

    Ok(())
}
