use async_trait::async_trait;
use common::api::def::{NodeBackendApi, NodeRunnerApi};
use common::api::rest::RestError;
use common::api::vfs::File;

#[cfg(any(test, not(target_env = "sgx")))]
pub mod mock;

mod client;

pub use client::*;

/// A trait for a client that can handle requests to both the backend + runner,
/// plus some methods to call into these services with retries.
#[async_trait]
pub trait ApiClient: NodeBackendApi + NodeRunnerApi {
    async fn create_file_with_retries(
        &self,
        file: &File,
        retries: usize,
    ) -> Result<File, RestError>;

    async fn upsert_file_with_retries(
        &self,
        file: &File,
        retries: usize,
    ) -> Result<File, RestError>;
}
