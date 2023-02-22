use std::sync::Arc;

#[cfg(any(not(target_env = "sgx"), test))]
use anyhow::ensure;
#[cfg(all(target_env = "sgx", not(test)))]
use anyhow::Context;
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
#[allow(unused_variables)] // `allow_mock` isn't read in sgx
pub(crate) fn new_backend_api(
    allow_mock: bool,
    maybe_backend_url: Option<String>,
) -> anyhow::Result<Arc<dyn BackendApiClient + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real backend client in production (sgx)
            let backend_url = maybe_backend_url
                .context("--backend-url must be supplied in production")?;
            Ok(Arc::new(client::BackendClient::new(backend_url)))
        } else {
            // Can use real OR mock client during development
            match maybe_backend_url {
                Some(backend_url) =>
                    Ok(Arc::new(client::BackendClient::new(backend_url))),
                None => {
                    ensure!(
                        allow_mock,
                        "Backend url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockBackendClient::new()))
                }
            }
        }
    }
}

/// Helper to initiate a client to the LSP.
#[allow(unused_variables)] // `allow_mock` isn't read in sgx
pub(crate) fn new_lsp_api(
    allow_mock: bool,
    maybe_lsp_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeLspApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real lsp client in production (sgx)
            let lsp_url = maybe_lsp_url
                .context("LspInfo's url field must be Some(_) in production")?;
            Ok(Arc::new(client::LspClient::new(lsp_url)))
        } else {
            // Can use real OR mock client during development
            match maybe_lsp_url {
                Some(lsp_url) =>
                    Ok(Arc::new(client::LspClient::new(lsp_url))),
                None => {
                    ensure!(
                        allow_mock,
                        "LSP url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockLspClient))
                }
            }
        }
    }
}

/// Helper to initiate a client to the runner.
#[allow(unused_variables)] // `allow_mock` isn't read in sgx
pub(crate) fn new_runner_api(
    allow_mock: bool,
    maybe_runner_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeRunnerApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_env = "sgx", not(test)))] {
            // Can only use the real runner client in production (sgx)
            let runner_url = maybe_runner_url
                .context("--runner-url must be supplied in production")?;
            Ok(Arc::new(client::RunnerClient::new(runner_url)))
        } else {
            // Can use real OR mock client during development
            match maybe_runner_url {
                Some(runner_url) =>
                    Ok(Arc::new(client::RunnerClient::new(runner_url))),
                None => {
                    ensure!(
                        allow_mock,
                        "Runner url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockRunnerClient::new()))
                }
            }
        }
    }
}
