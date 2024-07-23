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

pub use flutter_rust_bridge as frb;

/// The top-level App state
pub mod app;
/// The flutter/rust FFI bindings.
pub(crate) mod ffi;
/// `FlatFileFs` and `Ffs`.
mod ffs;
/// Flutter/rust ffi bindings generated from `ffi` by `just app-rs-codegen`.
pub(crate) mod frb_generated;
/// Pipe `tracing` log messages from native Rust to Dart.
mod logger;
/// App-local payment db and payment sync from node.
pub(crate) mod payments;
/// Securely store and retrieve user credentials to and from each platform's
/// standard secret storage.
pub mod secret_store;
/// Settings DB
mod settings;
/// Misc utilities related to local app storage.
pub mod storage;
