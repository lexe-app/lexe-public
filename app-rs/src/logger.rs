//! Pipe [`tracing`] log messages from native Rust to Dart.

use std::{
    fmt::{self, Write},
    str::FromStr,
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use lexe_api::{define_trace_id_fns, trace};
use tracing::{field, span, Event, Level, Subscriber};
use tracing_subscriber::{
    filter::{Filtered, Targets},
    layer::{Context, Layer, Layered, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    util::SubscriberInitExt,
    Registry,
};

use crate::logger::atomic_log_ptr::{AtomicLogFnPtr, LogFnPtr};

/// Just drop log messages until the logger is init'd.
fn noop_log(_message: String) {}

/// A function/callback to log messages from rust. Formatted rust log messages
/// are sent typically sent across a channel for printing on the dart side.
///
/// We use an `AtomicLogFnPtr` here because we need to be able to reset the fn
/// after each flutter hot restart.
static RUST_LOG_FN: AtomicLogFnPtr = AtomicLogFnPtr::new(noop_log);

/// An impl of `tracing_subscriber::Layer` that formats log messages and
/// forwards them into [`RUST_LOG_FN`].
struct DartLogLayer;

/// Span fields are formatted when an enabled span is first entered.
struct FormattedSpanFields {
    buf: String,
}

#[allow(dead_code)]
/// See `crate::ffi::ffi::init_rust_log_stream`.
pub(crate) fn init(log_fn: LogFnPtr, rust_log: &str) {
    RUST_LOG_FN.store(log_fn, Ordering::Relaxed);

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

        let log_fn = RUST_LOG_FN.load(Ordering::Relaxed);
        log_fn(message);
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

impl field::Visit for FieldVisitor<'_> {
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

/// An `AtomicPtr` specialized to store `fn(...)` pointer safely. Sadly Rust
/// traits aren't powerful enough yet to do this generically.
///
/// Let's stick this inside it's own module to prevent accidental misuse.
mod atomic_log_ptr {
    use std::{
        mem,
        sync::atomic::{AtomicPtr, Ordering},
    };

    use lexe_std::const_assert_usize_eq;

    pub(crate) type LogFnPtr = fn(message: String);

    // Sanity check the current platform. Ensure that its function pointers look
    // like pointers.
    const_assert_usize_eq!(
        mem::size_of::<LogFnPtr>(),
        mem::size_of::<*mut ()>(),
    );
    const_assert_usize_eq!(
        mem::align_of::<LogFnPtr>(),
        mem::align_of::<*mut ()>(),
    );

    #[repr(transparent)]
    pub(crate) struct AtomicLogFnPtr {
        inner: AtomicPtr<()>,
    }

    impl AtomicLogFnPtr {
        pub(crate) const fn new(fn_ptr: LogFnPtr) -> Self {
            Self {
                inner: AtomicPtr::new(fn_ptr as *mut ()),
            }
        }

        pub(crate) fn store(&self, fn_ptr: LogFnPtr, order: Ordering) {
            self.inner.store(fn_ptr as *mut (), order)
        }

        pub(crate) fn load(&self, order: Ordering) -> LogFnPtr {
            let fn_ptr_raw: *mut () = self.inner.load(order);
            // SAFETY: we ensure that we only put real function pointers in via
            // Self::new and Self::store, so this cannot be null or mismatched.
            let fn_ptr: LogFnPtr = unsafe { std::mem::transmute(fn_ptr_raw) };
            fn_ptr
        }
    }
}

// #[cfg(test)]
// mod test {
//     use lexe_api::trace::TraceId;
//     use flutter_rust_bridge::rust2dart::Rust2Dart;
//
//     use super::*;
//
//     #[test]
//     fn get_and_insert_trace_ids() {
//         let rust_log_tx = StreamSink::new(Rust2Dart::new(6969));
//         let rust_log = "INFO";
//         init(rust_log_tx, rust_log);
//         TraceId::get_and_insert_test_impl();
//     }
// }
