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

// Full app API:
//  GET  /app/node_info
//  GET  /app/list_channels
// POST  /app/sign_message
// POST  /app/verify_message
// POST  /app/open_channel
// POST  /app/preflight_open_channel
// POST  /app/close_channel
// POST  /app/preflight_close_channel
// POST  /app/create_invoice
// POST  /app/pay_invoice
// POST  /app/preflight_pay_invoice
// POST  /app/create_offer
// POST  /app/pay_offer
// POST  /app/preflight_pay_offer
// POST  /app/get_address
// POST  /app/pay_onchain
// POST  /app/preflight_pay_onchain
// POST  /app/payments/indexes
//  GET  /app/payments/new
//  PUT  /app/payments/note
//
// Basic prototype sdk API:
//  GET  /v1/health
//  GET  /v1/node/node_info
// POST  /v1/node/create_invoice
// POST  /v1/node/pay_invoice
//  GET  /v1/node/payment

// design decisions:
//
// - provide the sidecar as a
//   1. docker container (amd64, arm64)
//   2. standalone binary (x86_64, aarch64)
//
// - with fine-grained auth, the sidecar can't (re-)provision. it just has an
//   mTLS client cert that can connect to the node.
//
// - if we remove the gDrive requirement, a user could also operate without a
//   mobile app by generating the root seed locally and having the sidecar
//   attempt to signup on startup.
//
// - codegen an OpenAPI spec from the rust code to build our docs site and/or
//   client libraries
//
//   + Use `utoipa` / `utoipa-axum` crates
//     + ```rust
//     | #[derive(Serialize, Deserialize)]
//     | #[cfg_attr(feature = "openapi", derive(ToSchema))]`
//     | pub struct NodeInfo {
//     |     // ..
//     | }
//     | ```
//
//     + ```rust
//     | #[utoipa::path(get, path="/sdk/node_info")]
//     | async fn node_info(
//     |     State(state): State<Arc<SidecarState>>,
//     | ) -> Result<NodeInfo> {
//     |     // ..
//     | }
//     | ```
//
//   + use <https://scalar.com/> for API docs
//     + <https://www.npmjs.com/package/@scalar/api-reference>
//     + ```html
//     | <html>
//     | <head></head>
//     | <body>
//     |   <script id="api-reference" type="application/json">
//     |     /* the OpenAPI spec json */
//     |   </script>
//     |   <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
//     | </body>
//     | </html>
//     | ```
//
//   + plug openapi specs into postman
//     + <https://learning.postman.com/docs/integrations/available-integrations/working-with-openAPI/>
//
// - envs:
//   - (mandatory)
//   + NODE_CERTS=<base64-encoded root seed cert+key+CA json>
//
//   - (optional)
//   + SIDECAR_API_KEY=<path/api_key> (optional)
//   + LISTEN_ADDR=<ip:port> (default=[::1]:5393)
//   + NETWORK=<mainnet|testnet|regtest> (default=mainnet)
//   + DEPLOY_ENV=<prod|staging|dev> (default=prod)
//
// - future:
//   + fine-grained auth and non-root-seed derived mTLS to node so users don't
//     need to unsafely export their root seed.
//   + actual (python, typescript) language bindings that directly connect to
//     the node.
//
// usage:
//
// Run the sidecar as a standalone binary:
//
// ```bash
// $ cargo install lexe-sdk-sidecar
// $ NODE_CERTS=<..> lexe-sdk-sidecar
// ```
//
// Run the sidecar as a docker container:
//
// ```bash
// docker secret create node_certs "<..>"
//
// $ docker service create \
//     --name lexe-sdk-sidecar \
//     --secret node_certs \
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
//
// test:
//
// ```bash
// $ curl http://localhost:5393/node/node_info
// ```

pub mod cli;
pub mod run;
mod server;
