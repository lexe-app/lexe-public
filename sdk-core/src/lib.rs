//! Lexe SDK core types and traits shared across SDK interfaces.

#![deny(missing_docs)]

/// API trait definitions.
pub mod def;
/// API request and response types unique to a specific endpoint.
pub mod models;
/// Shared API types.
pub mod types;

// TODO(max): Replace these with LexeError
/// Temporary type alias for the errors returned by SDK APIs.
pub type SdkApiError = lexe_api_core::error::NodeApiError;
/// Temporary type alias for the error kinds returned by SDK APIs.
pub type SdkErrorKind = lexe_api_core::error::NodeErrorKind;
