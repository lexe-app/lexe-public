/// API request and response types exposed to Dart.
pub mod api;
/// The [`crate::app::App`] interface for top-level app state.
pub mod app;
/// High-level flutter/rust types and fns.
#[allow(clippy::module_inception)]
pub mod ffi;
/// Dart interface for app settings.
pub mod settings;
/// Data types to expose to Dart.
pub mod types;
