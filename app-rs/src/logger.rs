//! Pipe `tracing` log messages from native Rust to Dart.

#![allow(dead_code)]

use std::{
    fmt::{self, Write},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use arc_swap::ArcSwapOption;
use common::{api::trace, define_trace_id_fns};
use flutter_rust_bridge::StreamSink;
use tracing::{field, span, Event, Level, Subscriber};
use tracing_subscriber::{
    filter::{Filtered, Targets},
    layer::{Context, Layer, Layered, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    util::SubscriberInitExt,
    Registry,
};

/// A channel to dart. Formatted rust log messages are sent across this channel
/// for printing on the dart side.
static RUST_LOG_TX: ArcSwapOption<StreamSink<String>> =
    ArcSwapOption::const_empty();

struct DartLogLayer;

/// Span fields are formatted when an enabled span is first entered.
struct FormattedSpanFields {
    buf: String,
}

/// See [`crate::bindings::init_rust_log_stream`].
pub(crate) fn init(rust_log_tx: StreamSink<String>, rust_log: &str) {
    RUST_LOG_TX.store(Some(Arc::new(rust_log_tx)));

    let subscriber = subscriber(rust_log);

    // _DONT_ panic here if there is already a logger set. Instead we just
    // update the `RUST_LOG_TX`. We do this to support flutter hot reload.
    let _ = subscriber.try_init();
    define_trace_id_fns!(SubscriberType);
    let _ = trace::GET_TRACE_ID_FN.set(get_trace_id_from_span);
    let _ = trace::INSERT_TRACE_ID_FN.set(insert_trace_id_into_span);
}

/// The full type of our subscriber which is downcasted to when recovering
/// `TraceId`s. If having trouble naming this correctly, change this to some
/// dummy value (e.g. `u32`) and the compiler will tell you what it should be.
type SubscriberType =
    Layered<Filtered<DartLogLayer, Targets, Registry>, Registry>;

/// Generates our [`tracing::Subscriber`] impl. This function is extracted so
/// that we can check the correctness of the `SubscriberType` type alias, which
/// allows us to downcast back to our subscriber to recover `TraceId`s.
fn subscriber(rust_log: &str) -> SubscriberType {
    let rust_log_filter = Targets::from_str(rust_log)
        .ok()
        .unwrap_or_else(|| Targets::new().with_default(Level::INFO));

    let dart_log_layer = DartLogLayer::new().with_filter(rust_log_filter);

    tracing_subscriber::registry().with(dart_log_layer)
}

impl DartLogLayer {
    fn new() -> Self {
        Self
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for DartLogLayer {
    // When we enter into a new span, format the span fields and insert them
    // into this new span's extensions map.
    fn on_new_span(
        &self,
        attrs: &span::Attributes<'_>,
        id: &span::Id,
        ctx: Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("Span not found; this is a bug");
        let mut exts = span.extensions_mut();

        if exts.get_mut::<FormattedSpanFields>().is_none() {
            let mut fields = FormattedSpanFields { buf: String::new() };
            attrs.record(&mut FieldVisitor::new(&mut fields.buf));
            exts.insert(fields);
        }
    }

    // A new log event. Format the log event and send it over to dart.
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut message = String::new();
        fmt_event(&mut message, event, ctx).expect("Failed to format");

        RUST_LOG_TX.load().as_ref().map(|tx| tx.add(message));
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

    // metadata
    // ex: "1682371943.448209 R  INFO"
    write!(buf, "{timestamp:.06} R {level_pad}{level}")?;

    // span names
    // ex: " (app):(payments):(send): http:"
    let parent_span = event
        .parent()
        .and_then(|id| ctx.span(id))
        .or_else(|| ctx.lookup_current());
    fmt_span_names(buf, parent_span.as_ref())?;
    write!(buf, " {target}:")?;

    // event fields
    // ex: " done (success) status=200 time=1.3ms"
    event.record(&mut FieldVisitor::new(buf));

    // span fields
    // ex: " method=GET url=/node/get_payments"
    fmt_span_fields(buf, parent_span.as_ref())?;

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
fn fmt_span_names<S: Subscriber + for<'a> LookupSpan<'a>>(
    buf: &mut String,
    span: Option<&SpanRef<S>>,
) -> fmt::Result {
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

fn fmt_span_fields<S: Subscriber + for<'a> LookupSpan<'a>>(
    buf: &mut String,
    span: Option<&SpanRef<S>>,
) -> fmt::Result {
    let scope = span.into_iter().flat_map(|span| span.scope().from_root());

    for span in scope {
        let exts = span.extensions();
        if let Some(fields) = exts.get::<FormattedSpanFields>() {
            if !fields.buf.is_empty() {
                write!(buf, "{}", fields.buf)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use common::api::trace::TraceId;
    use flutter_rust_bridge::rust2dart::Rust2Dart;

    use super::*;

    #[test]
    fn get_and_insert_trace_ids() {
        let rust_log_tx = StreamSink::new(Rust2Dart::new(6969));
        let rust_log = "INFO";
        init(rust_log_tx, rust_log);
        TraceId::get_and_insert_test_impl();
    }
}
