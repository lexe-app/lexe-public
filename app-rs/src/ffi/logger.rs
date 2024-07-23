//! Rust logger integration.

use crate::frb_generated::StreamSink;

/// Init the Rust [`tracing`] logger. Also sets the current `RUST_LOG_TX`
/// instance, which ships Rust logs over to the dart side for printing.
///
/// Since `println!`/stdout gets swallowed on mobile, we ship log messages over
/// to dart for printing. Otherwise we can't see logs while developing.
///
/// When dart calls this function, it generates a `log_tx` and `log_rx`, then
/// sends the `log_tx` to Rust while holding on to the `log_rx`. When Rust gets
/// a new [`tracing`] log event, it enqueues the formatted log onto the
/// `log_tx`.
///
/// Unlike our other Rust loggers, this init will _not_ panic if a
/// logger instance is already set. Instead it will just update the
/// `RUST_LOG_TX`. This funky setup allows us to seamlessly support flutter's
/// hot restart, which would otherwise try to re-init the logger (and cause a
/// panic) but we still need to register a new log tx.
///
/// `rust_log`: since env vars don't work well on mobile, we need to ship the
/// equivalent of `$RUST_LOG` configured at build-time through here.
pub fn init_rust_log_stream(rust_log_tx: StreamSink<String>, rust_log: String) {
    use std::sync::Arc;

    use arc_swap::ArcSwapOption;

    /// A channel to send formatted log `String`s over to dart. We use an
    /// `ArcSwap` here since we need to be able to reset this after each flutter
    /// hot restart.
    static RUST_LOG_TX: ArcSwapOption<StreamSink<String>> =
        ArcSwapOption::const_empty();

    // Set the current log _tx_.
    RUST_LOG_TX.store(Some(Arc::new(rust_log_tx)));

    // Log fn that loads `RUST_LOG_TX` and tries to enqueue the message.
    fn rust_log_fn(message: String) {
        if let Some(rust_log_tx) = RUST_LOG_TX.load().as_ref() {
            // can return Err(..) if Dart side closes the stream for some reason
            let _ = rust_log_tx.add(message);
        }
    }

    // Set the current log _fn_.
    crate::logger::init(rust_log_fn, &rust_log);
}
