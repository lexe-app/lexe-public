//! # Rust/Dart FFI bindings
//!
//! ## TL;DR: REGENERATE THE BINDINGS
//!
//! If you update any sources in this directory, make sure to run:
//!
//! ```bash
//! $ just app-rs-codegen
//! # (alias)
//! $ j acg
//! ```
//!
//! ## Overview
//!
//! This directory contains all types and functions exposed to Dart. All `pub`
//! functions, structs, and enums in this directory will have corresponding
//! representations in the generated Dart code.
//!
//! The generated Dart interfaces live in
//! `app_rs_dart/lib/frb_generated.dart` (all generated dart impls) and
//! `app_rs_dart/ffi/<module>.dart` (each module's generated definitions) and
//! `app_rs_dart/ffi/<module>.freezed.dart` (each module's `freezed` codegen).
//!
//! The low-level generated Rust C-ABI interface is in [`crate::frb_generated`].
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
//! * Functions with `flutter_rust_bridge:sync` do block the calling Dart
//!   isolate and are run in-place on that isolate.
//! * `flutter_rust_bridge:sync` has ~10x less latency overhead. Think a few
//!   50-100 ns vs a few µs overhead per call.
//! * However, we have to be careful about blocking the main UI isolate, since
//!   we only have 16 ms frame budget to compute and render the UI without jank.
//!   Any sync ffi that runs for longer than maybe 1 ms should definitely run as
//!   a separate task on the threadpool. OTOH, just reading a value out of some
//!   in-memory state is probably cheaper overall to use
//!   `flutter_rust_bridge:sync`.

// TODO(phlip9): error messages need to be internationalized

/// API request and response types exposed to Dart.
pub mod api;
/// The [`crate::app::App`] interface for top-level app state.
pub mod app;
/// Dart interface for app data.
pub mod app_data;
/// Debug methods for use during development.
pub mod debug;
/// Form field validators.
pub mod form;
/// Google Drive OAuth2 + API
pub mod gdrive;
/// Rust logger integration.
pub mod logger;
/// QR code generation.
pub mod qr;
/// Dart interface to app secret store
pub mod secret_store;
/// Dart interface for app settings.
pub mod settings;
/// Data types to expose to Dart.
pub mod types;
