//! Crate containing Lexe API types, definitions, client/server utils, TLS.

/// Bearer auth and User Signup.
pub mod auth;
/// Traits defining Lexe's various APIs.
pub mod def;
/// Macros for API clients/servers.
pub mod macros;
/// API request and response types unique to a specific endpoint.
pub mod models;
/// A client and helpers that enforce common REST semantics across Lexe crates.
pub mod rest;
/// Webserver utilities.
pub mod server;
/// TLS certs and configurations.
pub mod tls;
/// API tracing utilities for both client and server.
pub mod trace;
/// Shared API types.
pub mod types;
