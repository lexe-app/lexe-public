//! Lexe SDK

use std::fmt;

use anyhow::Context;
use common::env::DeployEnv as DeployEnvRs;
use lexe_api_core::{def::AppGatewayApi, error::GatewayApiError};
use node_client::client::GatewayClient as GatewayClientRs;

uniffi::setup_scaffolding!("lexe");

#[uniffi::export]
fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[derive(uniffi::Enum)]
pub enum DeployEnv {
    Dev,
    Staging,
    Prod,
}

impl From<DeployEnv> for DeployEnvRs {
    fn from(env: DeployEnv) -> Self {
        match env {
            DeployEnv::Dev => Self::Dev,
            DeployEnv::Staging => Self::Staging,
            DeployEnv::Prod => Self::Prod,
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct FfiError {
    message: String,
}

impl std::error::Error for FfiError {}
impl From<anyhow::Error> for FfiError {
    fn from(err: anyhow::Error) -> Self {
        Self {
            message: format!("{err:#}"),
        }
    }
}
impl From<GatewayApiError> for FfiError {
    fn from(err: GatewayApiError) -> Self {
        Self {
            message: format!("{err:#}"),
        }
    }
}
impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub type FfiResult<T> = std::result::Result<T, FfiError>;

/// A client for interacting with the Lexe API Gateway.
#[derive(uniffi::Object)]
pub struct GatewayClient {
    inner: GatewayClientRs,
}

#[uniffi::export(async_runtime = "tokio")]
impl GatewayClient {
    /// Creates a new `GatewayClient` instance.
    #[uniffi::constructor]
    pub fn new(
        deploy_env: DeployEnv,
        gateway_url: String,
        user_agent: String,
    ) -> FfiResult<Self> {
        let deploy_env = DeployEnvRs::from(deploy_env);
        let client = GatewayClientRs::new(deploy_env, gateway_url, user_agent)?;
        Ok(Self { inner: client })
    }

    /// Fetches the latest available node enclave versions from the gateway.
    pub async fn latest_enclave(&self) -> FfiResult<String> {
        let enclaves = self.inner.current_enclaves().await?;
        let enclave = enclaves.latest().context("no latest enclave found")?;
        Ok(format!("{enclave:?}"))
    }
}
