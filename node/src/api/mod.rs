use async_trait::async_trait;
use thiserror::Error;

use crate::types::UserId;

mod client;
mod models;

pub use client::*;
pub use models::*;

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
}

#[async_trait]
pub trait ApiClient {
    // Having the constructor in the trait allows using `ApiClientType::new()`
    // without having to add cfg flags for the different new() APIs
    fn new(backend_url: String, runner_url: String) -> Self;

    async fn get_node(&self, user_id: UserId)
        -> Result<Option<Node>, ApiError>;
    async fn get_instance(
        &self,
        user_id: UserId,
        measurement: String,
    ) -> Result<Option<Instance>, ApiError>;

    async fn get_enclave(
        &self,
        user_id: UserId,
        measurement: String,
    ) -> Result<Option<Enclave>, ApiError>;

    async fn create_node_instance_enclave(
        &self,
        req: NodeInstanceEnclave,
    ) -> Result<NodeInstanceEnclave, ApiError>;

    async fn get_file(&self, file_id: FileId)
        -> Result<Option<File>, ApiError>;

    async fn create_file(&self, file: File) -> Result<File, ApiError>;

    async fn upsert_file(&self, file: File) -> Result<File, ApiError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: FileId) -> Result<String, ApiError>;

    async fn get_directory(
        &self,
        dir_id: DirectoryId,
    ) -> Result<Vec<File>, ApiError>;

    async fn notify_runner(
        &self,
        user_port: UserPort,
    ) -> Result<UserPort, ApiError>;
}
