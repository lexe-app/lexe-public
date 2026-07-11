//! # Lexe Sidecar SDK
//!
//! This crate contains the library code for the Lexe Sidecar SDK:
//! <https://github.com/lexe-app/lexe-sidecar-sdk>
//!
//! ## Overview
//!
//! The sidecar runs as a separate process and manages the connection to the
//! user's Lexe node. The sidecar handles provisioning new node versions, user
//! node auth, and connecting to the node via mTLS, while presenting a
//! simplified API to the SDK user.
//!
//! The sidecar is stateless, stores no data on-disk, and does no internal
//! caching. This helps reduce complexity and makes it easier to deploy and
//! manage.
//!
//! ## Why a sidecar?
//!
//! The sidecar lets us avoid building full language bindings / SDKs for each
//! language/platform we want to support. We don't currently have enough
//! engineering resources to tackle per-language SDKs, so we've opted for a
//! sidecar with a simplified REST API in the meantime.
//!
//! The sidecar also handles the tricky remote attestation process and
//! mTLS-in-TLS connection to the remote user node enclave.

// TODO(max): Consider shipping the sidecar as a Docker container (amd64, arm64)
//
// Run the sidecar as a docker container:
//
// ```bash
// docker secret create client_credentials "<..>"
//
// $ docker service create \
//     --name lexe-sdk-sidecar \
//     --secret client_credentials \
//     lexe/sdk-sidecar:latest
// ```
//
// Run as docker compose:
//
// ```yaml
// services:
//   lexe:
//     image: lexe/sdk-sidecar:latest
//     ports:
//       - "5393:5393"
// ```

// TODO(max): Consider `cargo install lexe-sidecar`

/// Sidecar-specific request and response types.
pub mod api;
/// Command-line interface.
pub mod cli;
/// `SidecarClient`.
pub mod client;
/// Sidecar API definition.
pub mod def;
/// The main `Sidecar` struct that is run.
pub mod run;
/// The sidecar webserver.
mod server;

/// Sidecar-specific axum extractors
mod extract;
/// Webhook types and functionality for payment notifications.
mod webhook;

// Reexport the Lexe SDK crate.
pub use lexe;
// Reexport types from internal crates that appear in the sidecar's public
// API.
pub use lexe_tokio::notify_once::NotifyOnce;
