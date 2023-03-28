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
//! - 3) Data used to make the request e.g. `VfsFileId`
//! - 4) The return type e.g. `Option<VfsFile>`
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
use crate::api::qs::GetRange;
use crate::api::vfs::{VfsDirectory, VfsFile, VfsFileId};
use crate::api::{NodePk, Scid, User, UserPk};
use crate::ed25519;
use crate::ln::invoice::LxInvoice;
use crate::ln::payments::{BasicPayment, DbPayment, LxPaymentId};

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
        auth: UserAuthToken,
    ) -> Result<Option<Scid>, BackendApiError>;

    /// GET /v1/file [`VfsFileId`] -> [`Option<VfsFile>`]
    async fn get_file(
        &self,
        file_id: &VfsFileId,
        auth: UserAuthToken,
    ) -> Result<Option<VfsFile>, BackendApiError>;

    /// POST /v1/file [`VfsFile`] -> [`()`]
    async fn create_file(
        &self,
        file: &VfsFile,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// PUT /v1/file [`VfsFile`] -> [`()`]
    async fn upsert_file(
        &self,
        file: &VfsFile,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// DELETE /v1/file [`VfsFileId`] -> [`()`]
    ///
    /// Returns [`Ok`] only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// GET /v1/directory [`VfsDirectory`] -> [`Vec<VfsFile>`]
    async fn get_directory(
        &self,
        dir: &VfsDirectory,
        auth: UserAuthToken,
    ) -> Result<Vec<VfsFile>, BackendApiError>;

    /// GET /v1/payments [`GetRange`] -> [`Vec<DbPayment>`]
    ///
    /// Fetches all payments within a `[start, end)` range.
    async fn get_payments(
        &self,
        range: GetRange,
        auth: UserAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError>;

    /// POST /v1/payments [`DbPayment`] -> [`()`]
    async fn create_payment(
        &self,
        payment: DbPayment,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// PUT /v1/payments [`DbPayment`] -> [`()`]
    async fn upsert_payment(
        &self,
        payment: DbPayment,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError>;

    /// GET /v1/payments/pending -> [`Vec<DbPayment>`]
    ///
    /// Fetches all pending payments.
    async fn get_pending_payments(
        &self,
        auth: UserAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError>;

    /// GET /v1/payments/final -> [`Vec<LxPaymentId>`]
    ///
    /// Fetches the IDs of all finalized payments.
    async fn get_finalized_payment_ids(
        &self,
        auth: UserAuthToken,
    ) -> Result<Vec<LxPaymentId>, BackendApiError>;
}

/// The user-facing backend APIs.
// TODO(max): Separate out the signup method into AppBackendApi, then rename to
// UserAuthBackendApi, with a comment explaining why this is so (because it's
// used in the UserAuthenticator)
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

    /// GET /owner/payments [`GetRange`] -> [`Vec<BasicPayment>`]
    async fn get_payments(
        &self,
        range: GetRange,
    ) -> Result<Vec<BasicPayment>, NodeApiError>;
}
