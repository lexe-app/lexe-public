use std::sync::Arc;

#[cfg(any(test, feature = "test-utils"))]
use anyhow::ensure;
use anyhow::Context;
use async_trait::async_trait;
use common::{
    api::{
        auth::BearerAuthToken,
        def::{
            BearerAuthBackendApi, NodeBackendApi, NodeLspApi, NodeRunnerApi,
        },
        error::BackendApiError,
        vfs::VfsFile,
        Empty,
    },
    env::DeployEnv,
};

/// Real clients.
pub(crate) mod client;
/// Mock clients.
#[cfg(any(test, feature = "test-utils"))]
pub mod mock;

/// A trait for a client that implements both backend API traits, plus a
/// method which allows the caller to specify the number of retries.
#[async_trait]
pub trait BackendApiClient: NodeBackendApi + BearerAuthBackendApi {
    async fn upsert_file_with_retries(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
        retries: usize,
    ) -> Result<Empty, BackendApiError>;
}

/// Helper to initiate a client to the backend.
pub(crate) fn new_backend_api(
    allow_mock: bool,
    maybe_backend_url: Option<String>,
) -> anyhow::Result<Arc<dyn BackendApiClient + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
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
        } else {
            // Can only use the real backend client in staging/prod
            let _ = allow_mock;
            let backend_url = maybe_backend_url
                .context("--backend-url must be supplied in staging/prod")?;
            Ok(Arc::new(client::BackendClient::new(backend_url)))
        }
    }
}

/// Helper to initiate a client to the LSP.
pub(crate) fn new_lsp_api(
    allow_mock: bool,
    deploy_env: DeployEnv,
    maybe_lsp_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeLspApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Can use real OR mock client during development
            match maybe_lsp_url {
                Some(lsp_url) => {
                    let lsp_client = client::LspClient::new(deploy_env, lsp_url)
                        .context("Could not init LspClient")?;
                    Ok(Arc::new(lsp_client))
                }
                None => {
                    ensure!(
                        allow_mock,
                        "LSP url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockLspClient))
                }
            }
        } else {
            // Can only use the real lsp client in staging/prod
            let _ = allow_mock;
            let _ = deploy_env;
            let lsp_url = maybe_lsp_url
                .context("LspInfo's url field must be Some(_) in staging/prod")?;
            let lsp_client = client::LspClient::new(deploy_env, lsp_url)
                .context("Could not init LspClient")?;
            Ok(Arc::new(lsp_client))
        }
    }
}

/// Helper to initiate a client to the runner.
pub(crate) fn new_runner_api(
    allow_mock: bool,
    deploy_env: DeployEnv,
    maybe_runner_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeRunnerApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Can use real OR mock client during development
            match maybe_runner_url {
                Some(runner_url) => {
                    let runner_client =
                        client::RunnerClient::new(deploy_env, runner_url)
                            .context("Failed to init RunnerClient")?;
                    Ok(Arc::new(runner_client))
                }
                None => {
                    anyhow::ensure!(
                        allow_mock,
                        "Runner url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockRunnerClient::new()))
                }
            }
        } else {
            // Can only use the real runner client in staging/prod
            let _ = allow_mock;
            let _ = deploy_env;
            let runner_url = maybe_runner_url
                .context("--runner-url must be supplied in staging/prod")?;
            let runner_client = client::RunnerClient::new(deploy_env, runner_url)
                .context("Failed to init RunnerClient")?;
            Ok(Arc::new(runner_client))
        }
    }
}
