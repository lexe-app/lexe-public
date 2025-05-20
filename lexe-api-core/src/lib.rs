//! Core Lexe API definitions, types and traits.

/// Traits defining Lexe's various APIs.
pub mod def;
/// API error types.
// TODO(max): This will be replaced by LexeError
pub mod error;
/// API request and response types unique to a specific endpoint.
pub mod models;
/// API types shared across multiple endpoints.
pub mod types;
/// Lexe's VFS ("virtual file system") trait and associated types.
pub mod vfs;

/// Axum helpers which must live in `lexe_api_core` because its dependents do.
#[cfg(feature = "axum")]
pub mod axum_helpers;
