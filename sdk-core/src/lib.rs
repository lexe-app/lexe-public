//! Lexe SDK core types and traits shared across SDK interfaces.

#![deny(missing_docs)]

/// API trait definitions.
pub mod def;
/// API request and response types unique to a specific endpoint.
pub mod models;
/// Shared API types.
pub mod types;

/// Temporary type alias for the errors returned by SDK APIs.
// TODO(max): Replace this with LexeError
pub type SdkApiError = lexe_api_core::error::NodeApiError;
