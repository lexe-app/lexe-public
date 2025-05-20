//! Core Lexe API definitions, types and traits.
//!
//! # Notes on API types
//!
//! ## Query parameters
//!
//! When serializing data as query parameters, we have to wrap newtypes in these
//! structs (instead of e.g. using UserPk directly), otherwise `serde_qs` errors
//! with "top-level serializer supports only maps and structs."
//!
//! ## `serde(flatten)`
//!
//! Also beware when using `#[serde(flatten)]` on a field. All inner fields must
//! be string-ish types (&str, String, Cow<'_, str>, etc...) OR use
//! `SerializeDisplay` and `DeserializeFromStr` from `serde_with`.
//!
//! This issue is due to a limitation in serde. See:
//! <https://github.com/serde-rs/serde/issues/1183>

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
