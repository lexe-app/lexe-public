use async_trait::async_trait;
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::Measurement;
use thiserror::Error;

#[cfg(any(test, not(target_env = "sgx")))]
pub mod mock;

mod client;

pub use client::*;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[error("JSON serialization error")]
    JsonSerialization(#[from] serde_json::Error),

    #[error("Query string serialization error")]
    QueryStringSerialization(#[from] serde_qs::Error),

    #[error("Server Error: {0}")]
    Server(String),

    #[error("Invalid response: {0}")]
    ResponseError(String),
}

#[async_trait]
pub trait ApiClient {
    async fn get_node(&self, user_pk: UserPk)
        -> Result<Option<Node>, ApiError>;

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, ApiError>;

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, ApiError>;

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, ApiError>;

    async fn get_file(
        &self,
        file_id: &FileId,
    ) -> Result<Option<File>, ApiError>;

    async fn create_file(&self, file: &File) -> Result<File, ApiError>;

    async fn upsert_file(&self, file: &File) -> Result<File, ApiError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: &FileId) -> Result<String, ApiError>;

    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, ApiError>;

    async fn notify_runner(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, ApiError>;
}
