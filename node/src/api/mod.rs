use async_trait::async_trait;
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use common::api::rest::RestError;
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::Measurement;

#[cfg(any(test, not(target_env = "sgx")))]
pub mod mock;

mod client;

pub use client::*;

/// A trait for a client that can handle requests to both the backend + runner
pub trait ApiClient: BackendService + RunnerService {}

impl<A: BackendService + RunnerService> ApiClient for A {}

#[async_trait]
pub trait BackendService {
    async fn get_node(&self, user_pk: UserPk)
        -> Result<Option<Node>, RestError>;

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, RestError>;

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, RestError>;

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, RestError>;

    async fn get_file(
        &self,
        file_id: &FileId,
    ) -> Result<Option<File>, RestError>;

    async fn create_file(&self, file: &File) -> Result<File, RestError>;

    async fn create_file_with_retries(
        &self,
        file: &File,
        retries: usize,
    ) -> Result<File, RestError>;

    async fn upsert_file(&self, file: &File) -> Result<File, RestError>;

    async fn upsert_file_with_retries(
        &self,
        file: &File,
        retries: usize,
    ) -> Result<File, RestError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: &FileId) -> Result<String, RestError>;

    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, RestError>;
}

#[async_trait]
pub trait RunnerService {
    async fn notify_runner(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RestError>;
}
