//! Pipe `tracing` log messages from native Rust to Dart.

#![allow(dead_code)]

use std::fmt::{self, Write};
use std::str::FromStr;

use flutter_rust_bridge::StreamSink;
use tracing::{field, Event, Level, Subscriber};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};

use crate::bindings::LogEntry;

struct DartLogLayer {
    rust_log_tx: StreamSink<LogEntry>,
}

pub(crate) fn init(rust_log_tx: StreamSink<LogEntry>) {
    try_init(rust_log_tx).expect("logger is already set!");
}

pub(crate) fn init_for_testing(rust_log_tx: StreamSink<LogEntry>) {
    // Quickly skip logger setup if no env var set.
    if std::env::var_os("RUST_LOG").is_none() {
        return;
    }

    // Don't panic if there's already a logger setup. Multiple tests might try
    // setting the global logger.
    let _ = try_init(rust_log_tx);
}

pub(crate) fn try_init(
    rust_log_tx: StreamSink<LogEntry>,
) -> Result<(), TryInitError> {
    let rust_log_filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|rust_log| Targets::from_str(&rust_log).ok())
        .unwrap_or_else(|| Targets::new().with_default(Level::INFO));

    let dart_log_layer = DartLogLayer::new(rust_log_tx);

    tracing_subscriber::registry()
        .with(dart_log_layer.with_filter(rust_log_filter))
        .try_init()
}

impl DartLogLayer {
    fn new(rust_log_tx: StreamSink<LogEntry>) -> Self {
        Self { rust_log_tx }
    }
}

impl<S: Subscriber> Layer<S> for DartLogLayer {
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut message = String::new();
        fmt_event(&mut message, event, ctx).expect("Failed to format");
        let log_entry = LogEntry { message };

        self.rust_log_tx.add(log_entry);
    }
}

fn fmt_event<S>(
    msg: &mut String,
    event: &Event<'_>,
    _ctx: Context<'_, S>,
) -> fmt::Result {
    let meta = event.metadata();
    let level = meta.level().as_str();

    // TODO(phlip9): display span stack
    // TODO(phlip9): display module, file, line?

    // pad INFO and WARN so log messages align
    if level.len() == 4 {
        msg.write_char(' ')?;
    }

    write!(msg, "{level}")?;
    event.record(&mut FieldVisitor::new(msg));
    Ok(())
}

struct FieldVisitor<W> {
    writer: W,
}

impl<W: fmt::Write> FieldVisitor<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: fmt::Write> field::Visit for FieldVisitor<W> {
    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "message" {
            self.record_debug(field, &format_args!("{}", value))
        } else {
            self.record_debug(field, &value)
        }
    }

    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        match field.name() {
            "message" => write!(self.writer, " {value:?}"),
            // skip `log` crate metadata
            name if name.starts_with("log.") => Ok(()),
            name => write!(self.writer, " {name}={value:?}"),
        }
        .expect("Failed to write??");
    }
}

// fn fmt_spans<W: Write>(w: &mut W) -> fmt::Result {
//     let mut seen = false;
//     let span = self
//         .span
//         .and_then(|id| self.ctx.ctx.span(id))
//         .or_else(|| self.ctx.ctx.lookup_current());
//
//     let scope = span.into_iter().flat_map(|span|
//     span.scope().from_root());
//
//     for span in scope {
//         seen = true;
//         write!(f, "{}:", bold.paint(span.metadata().name()))?;
//     }
//     if seen {
//         f.write_char(' ')?;
//     }
// }
