//! Crate containing Lexe API types, definitions, client/server utils, TLS.

/// Make all of [`lexe_api_core`] available under [`lexe_api`].
///
/// NOTE: Any crates which can depend on `lexe_api_core` directly (without
/// `lexe-api`) should do so to avoid [`lexe_api`] dependencies.
pub use lexe_api_core::*;

/// Bearer auth and User Signup.
pub mod auth;
/// Macros for API clients/servers.
pub mod macros;
/// A client and helpers that enforce common REST semantics across Lexe crates.
pub mod rest;
/// Webserver utilities.
pub mod server;
/// API tracing utilities for both client and server.
pub mod trace;

/// Feature-gated test utilities that can be shared across crate boundaries.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
