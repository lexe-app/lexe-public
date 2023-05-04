//! The native Rust code for the Lexe mobile app.

// For payments db node mocks
#![feature(btree_cursors)]
#![feature(once_cell)]
// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// Allow e.g. `CHANNEL_MANAGER` in generics to clearly distinguish between
// concrete and generic types
#![allow(non_camel_case_types)]
// Allow this in generated code
#![allow(clippy::not_unsafe_ptr_arg_deref)]

/// The top-level App state
pub mod app;
/// The high-level flutter/rust interface.
pub mod bindings;
/// The flutter/rust ffi bindings generated by `flutter_rust_bridge`.
pub mod bindings_generated;
/// The low-level handler `flutter_rust_bridge` calls to run dart tasks from the
/// ffi bridge.
mod dart_task_handler;
/// Pipe `tracing` log messages from native Rust to Dart.
mod logger;
/// App-local payment db and payment sync from node.
pub mod payments;
/// Securely store and retrieve user credentials to and from each platform's
/// standard secret storage.
pub mod secret_store;
