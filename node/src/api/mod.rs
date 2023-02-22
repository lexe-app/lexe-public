use std::sync::Arc;

use async_trait::async_trait;
use common::api::auth::UserAuthToken;
use common::api::def::{
    NodeBackendApi, NodeLspApi, NodeRunnerApi, UserBackendApi,
};
use common::api::error::BackendApiError;
use common::api::vfs::NodeFile;

/// Real clients.
pub(crate) mod client;
/// Mock clients.
#[cfg(any(test, not(target_env = "sgx")))]
pub mod mock;

/// A trait for a client that can handle requests to both the backend + runner,
/// plus some methods to call into these services with retries.
#[async_trait]
pub trait ApiClient:
    NodeBackendApi + NodeLspApi + NodeRunnerApi + UserBackendApi
{
    async fn create_file_with_retries(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<NodeFile, BackendApiError>;

    async fn upsert_file_with_retries(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<(), BackendApiError>;
}

/// A trait for a client that can implements both backend API traits, plus some
/// methods which allow the caller to specify the number of retries.
#[async_trait]
pub trait BackendApiClient: NodeBackendApi + UserBackendApi {
    async fn create_file_with_retries(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<NodeFile, BackendApiError>;

    async fn upsert_file_with_retries(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<(), BackendApiError>;
}

/// Helper to initiate a client to the backend.
#[allow(dead_code)] // TODO(max): Remove
#[allow(unused_variables)] // `mock` isn't read in sgx
pub(crate) fn new_backend_api(
    mock: bool,
    backend_url: String,
) -> Arc<dyn BackendApiClient + Send + Sync> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real backend client in production (sgx)
            Arc::new(client::BackendClient::new(backend_url))
        } else {
            // Can use real OR mock client during development
            if mock {
                Arc::new(mock::MockBackendClient::new())
            } else {
                Arc::new(client::BackendClient::new(backend_url))
            }
        }
    }
}

/// Helper to initiate a client to the LSP.
#[allow(dead_code)] // TODO(max): Remove
#[allow(unused_variables)] // `mock` isn't read in sgx
pub(crate) fn new_lsp_api(
    mock: bool,
    lsp_url: String,
) -> Arc<dyn NodeLspApi + Send + Sync> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real lsp client in production (sgx)
            Arc::new(client::LspClient::new(lsp_url))
        } else {
            // Can use real OR mock client during development
            if mock {
                Arc::new(mock::MockLspClient)
            } else {
                Arc::new(client::LspClient::new(lsp_url))
            }
        }
    }
}

/// Helper to initiate a client to the runner.
#[allow(dead_code)] // TODO(max): Remove
#[allow(unused_variables)] // `mock` isn't read in sgx
pub(crate) fn new_runner_api(
    mock: bool,
    runner_url: String,
) -> Arc<dyn NodeRunnerApi + Send + Sync> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real runner client in production (sgx)
            Arc::new(client::RunnerClient::new(runner_url))
        } else {
            // Can use real OR mock client during development
            if mock {
                Arc::new(mock::MockRunnerClient::new())
            } else {
                Arc::new(client::RunnerClient::new(runner_url))
            }
        }
    }
}
