//! # API Definitions
//!
//! This module, as closely as possible, defines the various APIs exposed by
//! different services to different clients. Although we do not have
//! compile-time guarantees that the services exposed exactly match the
//! definitions below, it is straightforward to compare the warp routes and
//! handlers with the definitions below to ensure consistency.
//!
//! ## Guidelines
//!
//! Each endpoint should be documented with:
//! - 1) HTTP method e.g. `GET`
//! - 2) Endpoint e.g. `/v1/file`
//! - 3) Data used to make the request e.g. `FileId`
//! - 4) The return type e.g. `Option<File>`
//!
//! The methods below should resemble the data actually sent across the wire.
//! Simple substitutions like `user_pk` instead of `GetByUserPk` are fine, but
//! the serialized data type (`GetByUserPk`) should be documented.

#![deny(missing_docs)]

use async_trait::async_trait;

use crate::api::error::{BackendApiError, NodeApiError, RunnerApiError};
use crate::api::node::{ListChannels, NodeInfo};
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
    /// GET /v1/node GetByUserPk -> Option<Node>
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, BackendApiError>;

    /// GET /v1/instance GetByUserPkAndMeasurement -> Option<Instance>
    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, BackendApiError>;

    /// GET /v1/sealed_seed SealedSeedId -> Option<SealedSeed>
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError>;

    /// POST /v1/node_instance_seed NodeInstanceSeed -> NodeInstanceSeed
    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, BackendApiError>;

    /// GET /v1/file File -> Option<File>
    async fn get_file(
        &self,
        file_id: &FileId,
    ) -> Result<Option<File>, BackendApiError>;

    /// POST /v1/file File -> File
    async fn create_file(&self, file: &File) -> Result<File, BackendApiError>;

    /// PUT /v1/file File -> File
    async fn upsert_file(&self, file: &File) -> Result<File, BackendApiError>;

    // TODO We want to delete channel peers / monitors when channels close
    /// DELETE /v1/file FileId -> "OK"
    /// Returns "OK" only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &FileId,
    ) -> Result<String, BackendApiError>;

    /// GET /v1/directory Directory -> Vec<File>
    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, BackendApiError>;
}

/// Defines the api that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    /// POST /ready UserPorts -> UserPorts
    async fn ready(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RunnerApiError>;
}

/// Defines the api that the node exposes to the host (Lexe)
#[async_trait]
pub trait HostNodeApi {
    /// GET /host/status GetByUserPk -> "OK"
    async fn status(&self, user_pk: UserPk) -> Result<String, NodeApiError>;
    /// GET /host/shutdown GetByUserPk -> ()
    async fn shutdown(&self, user_pk: UserPk) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during provisioning.
#[async_trait]
pub trait OwnerNodeProvisionApi {
    /// GET /provision ProvisionRequest -> PortReply
    async fn provision(
        &self,
        data: ProvisionRequest,
    ) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during normal operation.
#[async_trait]
pub trait OwnerNodeRunApi {
    /// GET /owner/node_info EmptyData -> NodeInfo
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError>;
    /// GET /owner/channels EmptyData -> ListChannels
    async fn list_channels(&self) -> Result<ListChannels, NodeApiError>;
}
