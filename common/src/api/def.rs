use async_trait::async_trait;

use crate::api::error::{BackendApiError, NodeApiError, RunnerApiError};
use crate::api::node::NodeInfo;
use crate::api::provision::{
    Instance, Node, NodeInstanceSeed, ProvisionRequest, SealedSeed,
    SealedSeedId,
};
use crate::api::runner::UserPorts;
use crate::api::vfs::{Directory, File, FileId};
use crate::api::UserPk;
use crate::enclave::Measurement;

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

/// Defines the api that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    async fn notify_runner(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RunnerApiError>;
}

/// Defines the api that the node exposes to the host (Lexe)
#[async_trait]
pub trait HostNodeApi {
    async fn status(&self, user_pk: UserPk) -> Result<String, NodeApiError>;
    async fn shutdown(&self, user_pk: UserPk) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during provisioning.
#[async_trait]
pub trait OwnerNodeProvisionApi {
    async fn provision(
        &self,
        data: ProvisionRequest,
    ) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during normal operation.
#[async_trait]
pub trait OwnerNodeRunApi {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError>;
}
