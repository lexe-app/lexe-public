use std::sync::Arc;

#[cfg(any(test, feature = "test-utils"))]
use anyhow::ensure;
use anyhow::Context;
use async_trait::async_trait;
use common::{
    api::auth::BearerAuthToken, env::DeployEnv, ln::network::LxNetwork,
    rng::Crng,
};
use lexe_api::{
    def::{BearerAuthBackendApi, NodeBackendApi, NodeLspApi, NodeRunnerApi},
    error::BackendApiError,
    types::Empty,
    vfs::VfsFile,
};
use lexe_ln::logger::LexeTracingLogger;
use lexe_tls::attestation::NodeMode;

/// Real clients.
pub(crate) mod client;
/// Mock clients.
#[cfg(any(test, feature = "test-utils"))]
pub mod mock;

/// The user agent string for external requests.
pub static USER_AGENT_EXTERNAL: &str = lexe_api::user_agent_external!();

/// A trait for a client that implements both backend API traits, plus a
/// method which allows the caller to specify the number of retries.
#[async_trait]
pub trait NodeBackendApiClient: NodeBackendApi + BearerAuthBackendApi {
    async fn upsert_file_with_retries(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
        retries: usize,
    ) -> Result<Empty, BackendApiError>;
}

/// Helper to initiate a client to the backend.
pub(crate) fn new_backend_api(
    rng: &mut impl Crng,
    allow_mock: bool,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
    maybe_backend_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeBackendApiClient + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Can use real OR mock client during development
            match maybe_backend_url {
                Some(backend_url) => {
                    let backend_client = client::NodeBackendClient::new(
                        rng, deploy_env, node_mode, backend_url
                    )
                    .context("Failed to init BackendClient")?;
                    Ok(Arc::new(backend_client))
                }
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
            let backend_client = client::NodeBackendClient::new(
                rng, deploy_env, node_mode, backend_url
            )
            .context("Failed to init BackendClient")?;
            Ok(Arc::new(backend_client))
        }
    }
}

/// Helper to initiate a client to the LSP.
pub(crate) fn new_lsp_api(
    rng: &mut impl Crng,
    allow_mock: bool,
    deploy_env: DeployEnv,
    network: LxNetwork,
    node_mode: NodeMode,
    maybe_lsp_url: Option<String>,
    logger: LexeTracingLogger,
) -> anyhow::Result<Arc<dyn NodeLspApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Can use real OR mock client during development
            match maybe_lsp_url {
                Some(lsp_url) => {
                    let lsp_client = client::NodeLspClient::new(
                        rng, deploy_env, node_mode, lsp_url
                    )
                    .context("Failed to init LspClient")?;
                    Ok(Arc::new(lsp_client))
                }
                None => {
                    ensure!(
                        allow_mock,
                        "LSP url not supplied, or --allow-mock wasn't set"
                    );
                    Ok(Arc::new(mock::MockLspClient { network, logger }))
                }
            }
        } else {
            // Can only use the real lsp client in staging/prod
            let _ = (allow_mock, deploy_env, network, logger);
            let lsp_url = maybe_lsp_url
                .context("LspInfo's url field must be Some(_) in staging/prod")?;
            let lsp_client =
                client::NodeLspClient::new(rng, deploy_env, node_mode, lsp_url)
                    .context("Failed to init LspClient")?;
            Ok(Arc::new(lsp_client))
        }
    }
}

/// Helper to initiate a client to the runner.
pub(crate) fn new_runner_api(
    rng: &mut impl Crng,
    allow_mock: bool,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
    maybe_runner_url: Option<String>,
) -> anyhow::Result<Arc<dyn NodeRunnerApi + Send + Sync>> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            // Can use real OR mock client during development
            match maybe_runner_url {
                Some(runner_url) => {
                    let runner_client = client::RunnerClient::new(
                        rng, deploy_env, node_mode, runner_url
                    )
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
            let runner_client = client::RunnerClient::new(
                rng, deploy_env, node_mode, runner_url
            )
            .context("Failed to init RunnerClient")?;
            Ok(Arc::new(runner_client))
        }
    }
}
