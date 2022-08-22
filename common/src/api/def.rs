use async_trait::async_trait;

use crate::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use crate::api::rest::RestError;
use crate::api::runner::UserPorts;
use crate::api::vfs::{Directory, File, FileId};
use crate::api::UserPk;
use crate::enclave::Measurement;

// TODO(max): This should return BackendApiError
/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, RestError>;

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

    async fn upsert_file(&self, file: &File) -> Result<File, RestError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: &FileId) -> Result<String, RestError>;

    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, RestError>;
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
