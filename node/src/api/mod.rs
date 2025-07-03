use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use common::{api::auth::BearerAuthToken, env::DeployEnv, rng::Crng};
use lexe_api::{
    def::{
        BearerAuthBackendApi, MegaRunnerApi, NodeBackendApi, NodeLspApi,
        NodeRunnerApi,
    },
    error::BackendApiError,
    types::Empty,
    vfs::VfsFile,
};
use lexe_tls::attestation::NodeMode;

/// Real clients.
pub(crate) mod client;

/// The user agent string for external requests.
pub static USER_AGENT_EXTERNAL: &str = lexe_api::user_agent_external!();

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

/// A trait for a client that implements both NodeRunnerApi and MegaRunnerApi.
#[async_trait]
pub trait RunnerApiClient: NodeRunnerApi + MegaRunnerApi {}

/// Helper to initiate a client to the backend.
pub(crate) fn new_backend_api(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
    backend_url: String,
) -> anyhow::Result<Arc<dyn BackendApiClient + Send + Sync>> {
    let backend_client =
        client::NodeBackendClient::new(rng, deploy_env, node_mode, backend_url)
            .context("Failed to init BackendClient")?;
    Ok(Arc::new(backend_client))
}

/// Helper to initiate a client to the LSP.
pub(crate) fn new_lsp_api(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
    lsp_url: String,
) -> anyhow::Result<Arc<dyn NodeLspApi + Send + Sync>> {
    let lsp_client =
        client::NodeLspClient::new(rng, deploy_env, node_mode, lsp_url)
            .context("Failed to init LspClient")?;
    Ok(Arc::new(lsp_client))
}

/// Helper to initiate a client to the runner.
pub(crate) fn new_runner_api(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
    runner_url: String,
) -> anyhow::Result<Arc<dyn RunnerApiClient + Send + Sync>> {
    let runner_client =
        client::RunnerClient::new(rng, deploy_env, node_mode, runner_url)
            .context("Failed to init RunnerClient")?;
    Ok(Arc::new(runner_client))
}
