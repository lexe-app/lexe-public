//! # Rust/Dart FFI bindings
//!
//! ## TL;DR: REGENERATE THE BINDINGS
//!
//! If you update sources in this directory, re-run:
//!
//! ```bash
//! $ just app-rs-codegen
//! # (alias)
//! $ j acg
//! ```
//!
//! ## Overview
//!
//! This file contains all types and functions exposed to Dart. All `pub`
//! functions, structs, and enums in this file also have corresponding
//! representations in the generated Dart code.
//!
//! The generated Dart interface lives in
//! `../../app/lib/app_rs/frb_generated.dart` (all generated dart impls) and
//! `../../app/lib/app_rs/ffi/ffi.dart` (this file's generated definitions) and
//! `../../app/lib/app_rs/ffi/ffi.freezed.dart` (this file's dart `freezed`
//! codegen).
//!
//! The low-level generated Rust C-ABI interface is in
//! [`crate::ffi::ffi_generated`].
//!
//! These FFI bindings are generated using the `app-rs-codegen` crate. Be sure
//! to re-run the `app-rs-codegen` whenever this file changes.
//!
//! ## Understanding the codegen
//!
//! * Both platforms have different representations for most common types like
//!   usize and String.
//! * Basic types are copied to the native platform representation when crossing
//!   the FFI boundary.
//! * For example strings are necessarily copied, as Rust uses utf-8 encoded
//!   strings while Dart uses utf-16 encoded strings.
//! * There are a few special cases where we can avoid copying, like returning a
//!   `Vec<u8>` from Rust, which becomes a `Uint8List` on the Dart side without
//!   a copy, since Rust can prove there are no borrows to the owned buffer when
//!   it's transferred.
//! * Normal looking pub functions, like `pub fn x() -> u32 { 123 }` look like
//!   async fn's on the Dart side and are run on a separate threadpool on the
//!   Rust side to avoid blocking the main Flutter UI isolate.
//! * Functions with `#[frb(sync)]` do block the calling Dart isolate and are
//!   run in-place on that isolate.
//! * `#[frb(sync)]` has ~10x less latency overhead. Think a few 50-100 ns vs a
//!   few µs overhead per call.
//! * However, we have to be careful about blocking the main UI isolate, since
//!   we only have 16 ms frame budget to compute and render the UI without jank.
//!   Any sync ffi that runs for longer than maybe 1 ms should definitely run as
//!   a separate task on the threadpool. OTOH, just reading a value out of some
//!   in-memory state is probably cheaper overall to use `#[frb(sync)]`.

use anyhow::Context;
use common::password;
use flutter_rust_bridge::frb;
use secrecy::Zeroize;

use crate::{
    app::AppConfig,
    ffi::types::{Config, Network, PaymentMethod},
    ffs::FlatFileFs,
    form,
    frb_generated::StreamSink,
    secret_store::SecretStore,
    storage,
};

#[rustfmt::skip]
// pub(crate) static FLUTTER_RUST_BRIDGE_HANDLER: LazyLock<LxHandler> =
//     LazyLock::new(|| {
//         // TODO(phlip9): Get backtraces symbolizing correctly on mobile. I'm at
//         // a bit of a loss as to why I can't get this working...
// 
//         // std::env::set_var("RUST_BACKTRACE", "1");
// 
//         // TODO(phlip9): If we want backtraces from panics, we'll need to set a
//         // custom panic handler here that formats the backtrace into the panic
//         // message string instead of printing it out to stderr (since mobile
//         // doesn't show stdout/stderr...)
// 
//         let error_handler = ReportDartErrorHandler;
//         LxHandler::new(ThreadPoolExecutor::new(error_handler), error_handler)
//     });

// #[frb(init)]
// pub fn init_app_rs() {
//     // When is this called?
//     // setup backtrace
//     // setup log
//     // flutter_rust_bridge::Handler
// }

// TODO(phlip9): error messages need to be internationalized

/// Validate whether `address_str` is a properly formatted bitcoin address. Also
/// checks that it's valid for the configured bitcoin network.
///
/// The return type is a bit funky: `Option<String>`. `None` means
/// `address_str` is valid, while `Some(msg)` means it is not (with given
/// error message). We return in this format to better match the flutter
/// `FormField` validator API.
#[frb(sync)]
pub fn form_validate_bitcoin_address(
    address_str: String,
    current_network: Network,
) -> Option<String> {
    let result =
        form::validate_bitcoin_address(&address_str, current_network.into());
    match result {
        Ok(()) => None,
        Err(msg) => Some(msg),
    }
}

/// Validate whether `password` has an appropriate length.
///
/// The return type is a bit funky: `Option<String>`. `None` means
/// `address_str` is valid, while `Some(msg)` means it is not (with given
/// error message). We return in this format to better match the flutter
/// `FormField` validator API.
#[frb(sync)]
pub fn form_validate_password(mut password: String) -> Option<String> {
    let result = password::validate_password_len(&password);
    password.zeroize();
    match result {
        Ok(()) => None,
        Err(err) => Some(err.to_string()),
    }
}

/// Resolve a (possible) [`PaymentUri`] string that we just
/// scanned/pasted into the best [`PaymentMethod`] for us to pay.
///
/// [`PaymentUri`]: payment_uri::PaymentUri
pub fn payment_uri_resolve_best(
    network: Network,
    uri_str: String,
) -> anyhow::Result<PaymentMethod> {
    payment_uri::PaymentUri::parse(&uri_str)
        .context("Unrecognized payment code")?
        .resolve_best(network.into())
        .map(PaymentMethod::from)
}

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

/// Delete the local persisted `SecretStore` and `RootSeed`.
///
/// WARNING: you will need a backup recovery to use the account afterwards.
#[frb(sync)]
pub fn debug_delete_secret_store(config: Config) -> anyhow::Result<()> {
    SecretStore::new(&config.into()).delete()
}

/// Delete the local latest_release file.
#[frb(sync)]
pub fn debug_delete_latest_provisioned(config: Config) -> anyhow::Result<()> {
    let app_config = AppConfig::from(config);
    let app_data_ffs = FlatFileFs::create_dir_all(app_config.app_data_dir)
        .context("Could not create app data ffs")?;
    storage::delete_latest_provisioned(&app_data_ffs)?;
    Ok(())
}

/// Unconditionally panic (for testing).
pub fn debug_unconditional_panic() {
    panic!("Panic inside app-rs");
}

/// Unconditionally return Err (for testing).
pub fn debug_unconditional_error() -> anyhow::Result<()> {
    Err(anyhow::format_err!("Error inside app-rs"))
}
