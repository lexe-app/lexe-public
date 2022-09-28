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
//! - 3) Data used to make the request e.g. `NodeFileId`
//! - 4) The return type e.g. `Option<NodeFile>`
//!
//! The methods below should resemble the data actually sent across the wire.

#![deny(missing_docs)]

use async_trait::async_trait;

use crate::api::error::{BackendApiError, NodeApiError, RunnerApiError};
use crate::api::node::{ListChannels, NodeInfo};
use crate::api::provision::{
    Instance, Node, NodeInstanceSeed, NodeProvisionRequest, SealedSeed,
    SealedSeedId,
};
use crate::api::runner::UserPorts;
use crate::api::vfs::{NodeDirectory, NodeFile, NodeFileId};
use crate::api::UserPk;
use crate::enclave::Measurement;

/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    /// GET /v1/node [`GetByUserPk`] -> [`Option<Node>`]
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, BackendApiError>;

    /// GET /v1/instance [`GetByUserPkAndMeasurement`] -> [`Option<Instance>`]
    ///
    /// [`GetByUserPkAndMeasurement`]: super::qs::GetByUserPkAndMeasurement
    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, BackendApiError>;

    /// GET /v1/sealed_seed [`SealedSeedId`] -> [`Option<SealedSeed>`]
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError>;

    /// POST /v1/node_instance_seed [`NodeInstanceSeed`] -> [`NodeInstanceSeed`]
    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, BackendApiError>;

    /// GET /v1/file [`NodeFileId`] -> [`Option<NodeFile>`]
    async fn get_file(
        &self,
        file_id: &NodeFileId,
    ) -> Result<Option<NodeFile>, BackendApiError>;

    /// POST /v1/file [`NodeFile`] -> [`NodeFile`]
    async fn create_file(
        &self,
        file: &NodeFile,
    ) -> Result<NodeFile, BackendApiError>;

    /// PUT /v1/file [`NodeFile`] -> [`NodeFile`]
    async fn upsert_file(
        &self,
        file: &NodeFile,
    ) -> Result<NodeFile, BackendApiError>;

    /// DELETE /v1/file [`NodeFileId`] -> "OK"
    ///
    /// Returns "OK" only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &NodeFileId,
    ) -> Result<String, BackendApiError>;

    /// GET /v1/directory [`NodeDirectory`] -> [`Vec<NodeFile>`]
    async fn get_directory(
        &self,
        dir: &NodeDirectory,
    ) -> Result<Vec<NodeFile>, BackendApiError>;
}

/// Defines the api that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    /// POST /ready [`UserPorts`] -> [`UserPorts`]
    async fn ready(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RunnerApiError>;
}

/// Defines the api that the node exposes to the host (Lexe)
#[async_trait]
pub trait HostNodeApi {
    /// GET /host/status [`GetByUserPk`] -> "OK"
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn status(&self, user_pk: UserPk) -> Result<String, NodeApiError>;

    /// GET /host/shutdown [`GetByUserPk`] -> [`()`]
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn shutdown(&self, user_pk: UserPk) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during provisioning.
#[async_trait]
pub trait OwnerNodeProvisionApi {
    /// GET /provision [`NodeProvisionRequest`] -> [`()`]
    async fn provision(
        &self,
        data: NodeProvisionRequest,
    ) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the owner during normal operation.
#[async_trait]
pub trait OwnerNodeRunApi {
    /// GET /owner/node_info [`EmptyData`] -> [`NodeInfo`]
    ///
    /// [`EmptyData`]: super::qs::EmptyData
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError>;

    /// GET /owner/channels [`EmptyData`] -> [`ListChannels`]
    ///
    /// [`EmptyData`]: super::qs::EmptyData
    async fn list_channels(&self) -> Result<ListChannels, NodeApiError>;
}
