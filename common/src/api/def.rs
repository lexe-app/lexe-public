use async_trait::async_trait;
use http::response::Response;
use http::status::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use warp::hyper::Body;
use warp::Reply;

use crate::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use crate::api::rest::RestError;
use crate::api::runner::UserPorts;
use crate::api::vfs::{Directory, File, FileId};
use crate::api::UserPk;
use crate::enclave::Measurement;
use crate::hex;

/// All errors that the backend can return.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum BackendApiError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Not found")]
    NotFound,
    #[error("Could not convert entity to type: {0}")]
    EntityConversion(String),
    #[error("Rest error: {0}")]
    Rest(#[from] RestError),
}

impl Reply for BackendApiError {
    fn into_response(self) -> Response<Body> {
        let err_str = format!("{:#}", self);
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(err_str.into())
            .expect("Could not construct Response")
    }
}

// Don't want the node to depend on sea-orm via the common crate
#[cfg(not(target_env = "sgx"))]
impl From<sea_orm::DbErr> for BackendApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Database(format!("{err:#}"))
    }
}
impl From<bitcoin::secp256k1::Error> for BackendApiError {
    fn from(err: bitcoin::secp256k1::Error) -> Self {
        Self::EntityConversion(format!("Pubkey decode err: {err:#}"))
    }
}
impl From<hex::DecodeError> for BackendApiError {
    fn from(err: hex::DecodeError) -> Self {
        Self::EntityConversion(format!("Hex decode error: {err:#}"))
    }
}

/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, BackendApiError>;

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, BackendApiError>;

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError>;

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, BackendApiError>;

    async fn get_file(
        &self,
        file_id: &FileId,
    ) -> Result<Option<File>, BackendApiError>;

    async fn create_file(&self, file: &File) -> Result<File, BackendApiError>;

    async fn upsert_file(&self, file: &File) -> Result<File, BackendApiError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &FileId,
    ) -> Result<String, BackendApiError>;

    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, BackendApiError>;
}

// TODO(max): This should return RunnerApiError
/// Defines the service that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    async fn notify_runner(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RestError>;
}
