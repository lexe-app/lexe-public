use async_trait::async_trait;
use common::api::auth::UserAuthToken;
use common::api::def::{
    NodeBackendApi, NodeLspApi, NodeRunnerApi, UserBackendApi,
};
use common::api::error::BackendApiError;
use common::api::vfs::NodeFile;

#[cfg(any(test, not(target_env = "sgx")))]
pub mod mock;

mod client;

pub use client::*;

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
    ) -> Result<NodeFile, BackendApiError>;
}
