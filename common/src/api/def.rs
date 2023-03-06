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

use crate::api::auth::{
    UserAuthRequest, UserAuthResponse, UserAuthToken, UserSignupRequest,
};
use crate::api::command::{GetInvoiceRequest, ListChannels, NodeInfo};
use crate::api::error::{
    BackendApiError, LspApiError, NodeApiError, RunnerApiError,
};
use crate::api::ports::UserPorts;
use crate::api::provision::{NodeProvisionRequest, SealedSeed, SealedSeedId};
use crate::api::vfs::{NodeDirectory, NodeFile, NodeFileId};
use crate::api::{NodePk, Scid, User, UserPk};
use crate::ed25519;
use crate::ln::invoice::LxInvoice;

/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    /// GET /v1/user [`GetByUserPk`] -> [`Option<User>`]
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn get_user(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<User>, BackendApiError>;

    /// GET /v1/sealed_seed [`SealedSeedId`] -> [`Option<SealedSeed>`]
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError>;

    /// POST /v1/sealed_seed [`SealedSeed`] -> [`()`]
    async fn create_sealed_seed(
        &self,
        data: SealedSeed,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// GET /v1/scid [`GetByNodePk`] -> [`Option<Scid>`]
    ///
    /// [`GetByNodePk`]: super::qs::GetByNodePk
    async fn get_scid(
        &self,
        node_pk: NodePk,
    ) -> Result<Option<Scid>, BackendApiError>;

    /// GET /v1/file [`NodeFileId`] -> [`Option<NodeFile>`]
    async fn get_file(
        &self,
        file_id: &NodeFileId,
        auth: UserAuthToken,
    ) -> Result<Option<NodeFile>, BackendApiError>;

    /// POST /v1/file [`NodeFile`] -> [`NodeFile`]
    async fn create_file(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
    ) -> Result<NodeFile, BackendApiError>;

    /// PUT /v1/file [`NodeFile`] -> [`()`]
    async fn upsert_file(
        &self,
        file: &NodeFile,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// DELETE /v1/file [`NodeFileId`] -> "OK"
    ///
    /// Returns "OK" only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &NodeFileId,
        auth: UserAuthToken,
    ) -> Result<String, BackendApiError>;

    /// GET /v1/directory [`NodeDirectory`] -> [`Vec<NodeFile>`]
    async fn get_directory(
        &self,
        dir: &NodeDirectory,
        auth: UserAuthToken,
    ) -> Result<Vec<NodeFile>, BackendApiError>;
}

/// The user-facing backend APIs.
#[async_trait]
pub trait UserBackendApi {
    /// POST /signup [`ed25519::Signed<UserSignupRequest>`] -> [`()`]
    async fn signup(
        &self,
        signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError>;

    /// POST /user_auth [`ed25519::Signed<UserAuthRequest>`]
    ///              -> [`UserAuthResponse`]
    async fn user_auth(
        &self,
        signed_req: ed25519::Signed<UserAuthRequest>,
    ) -> Result<UserAuthResponse, BackendApiError>;
}

/// Defines the api that the LSP exposes to user nodes.
#[async_trait]
pub trait NodeLspApi {
    /// GET /v1/scid [`GetByNodePk`] -> [`Option<Scid>`]
    ///
    /// [`GetByNodePk`]: super::qs::GetByNodePk
    async fn get_new_scid(&self, node_pk: NodePk) -> Result<Scid, LspApiError>;
}

/// Defines the api that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    /// POST /node/ready [`UserPorts`] -> [`UserPorts`]
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
    /// POST /provision [`NodeProvisionRequest`] -> [`()`]
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

    /// POST /owner/get_invoice [`GetInvoiceRequest`] -> [`ListChannels`]
    async fn get_invoice(
        &self,
        req: GetInvoiceRequest,
    ) -> Result<LxInvoice, NodeApiError>;

    /// POST /owner/send_payment [`LxInvoice`] -> [`()`]
    async fn send_payment(&self, req: LxInvoice) -> Result<(), NodeApiError>;
}
