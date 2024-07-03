//! The native Rust code for the Lexe mobile app.

// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]
// Allow this in generated code
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// TODO(phlip9): uncomment when I actually need this
// /// Android Context and JVM handle.
// #[cfg(target_os = "android")]
// pub(crate) mod android;

/// The top-level App state
pub mod app;
// /// The low-level handler `flutter_rust_bridge` calls to run dart tasks from
// the /// ffi bridge.
// mod dart_task_handler;
/// The flutter/rust FFI bindings.
mod ffi;
/// `FlatFileFs` and `Ffs`.
mod ffs;
/// UI form input helpers.
mod form;
/// Pipe `tracing` log messages from native Rust to Dart.
mod logger;
/// App-local payment db and payment sync from node.
pub(crate) mod payments;
/// Securely store and retrieve user credentials to and from each platform's
/// standard secret storage.
pub mod secret_store;
/// Misc utilities related to local app storage.
pub mod storage;
