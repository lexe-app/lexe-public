//! Crate containing Lexe API types, definitions, client/server utils, TLS.

/// Macros for API clients/servers.
pub mod macros;
/// A client and helpers that enforce common REST semantics across Lexe crates.
pub mod rest;
/// Webserver utilities.
pub mod server;
/// TLS certs and configurations.
pub mod tls;
/// API tracing utilities for both client and server.
pub mod trace;
/// API types.
pub mod types;
