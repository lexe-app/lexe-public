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
use bitcoin::Address;

use super::{error::GatewayApiError, fiat_rates::FiatRates};
use crate::{
    api::{
        auth::{
            BearerAuthRequest, BearerAuthResponse, BearerAuthToken,
            UserSignupRequest,
        },
        command::{
            CreateInvoiceRequest, NodeInfo, PayInvoiceRequest,
            SendOnchainRequest,
        },
        error::{BackendApiError, LspApiError, NodeApiError, RunnerApiError},
        ports::UserPorts,
        provision::{NodeProvisionRequest, SealedSeed, SealedSeedId},
        qs::{
            GetNewPayments, GetPaymentByIndex, GetPaymentsByIds,
            UpdatePaymentNote,
        },
        vfs::{VfsDirectory, VfsFile, VfsFileId},
        NodePk, Scid, User, UserPk,
    },
    ed25519,
    ln::{
        hashes::LxTxid,
        invoice::LxInvoice,
        payments::{BasicPayment, DbPayment, LxPaymentId},
    },
};

/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    /// GET /node/v1/user [`GetByUserPk`] -> [`Option<User>`]
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn get_user(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<User>, BackendApiError>;

    /// GET /node/v1/sealed_seed [`SealedSeedId`] -> [`Option<SealedSeed>`]
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError>;

    /// POST /node/v1/sealed_seed [`SealedSeed`] -> [`()`]
    async fn create_sealed_seed(
        &self,
        data: SealedSeed,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// GET /node/v1/scid [`GetByNodePk`] -> [`Option<Scid>`]
    ///
    /// [`GetByNodePk`]: super::qs::GetByNodePk
    async fn get_scid(
        &self,
        node_pk: NodePk,
        auth: BearerAuthToken,
    ) -> Result<Option<Scid>, BackendApiError>;

    /// GET /node/v1/file [`VfsFileId`] -> [`Option<VfsFile>`]
    async fn get_file(
        &self,
        file_id: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<Option<VfsFile>, BackendApiError>;

    /// POST /node/v1/file [`VfsFile`] -> [`()`]
    async fn create_file(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// PUT /node/v1/file [`VfsFile`] -> [`()`]
    async fn upsert_file(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// DELETE /node/v1/file [`VfsFileId`] -> [`()`]
    ///
    /// Returns [`Ok`] only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// GET /node/v1/directory [`VfsDirectory`] -> [`Vec<VfsFile>`]
    async fn get_directory(
        &self,
        dir: &VfsDirectory,
        auth: BearerAuthToken,
    ) -> Result<Vec<VfsFile>, BackendApiError>;

    /// GET /node/v1/payments [`GetPaymentByIndex`] -> [`Option<DbPayment>`]
    async fn get_payment(
        &self,
        req: GetPaymentByIndex,
        auth: BearerAuthToken,
    ) -> Result<Option<DbPayment>, BackendApiError>;

    /// POST /node/v1/payments [`DbPayment`] -> [`()`]
    async fn create_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// PUT /node/v1/payments [`DbPayment`] -> [`()`]
    async fn upsert_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// PUT /node/v1/payments/batch [`Vec<DbPayment>`] -> [`()`]
    ///
    /// ACID endpoint for upserting a batch of payments.
    async fn upsert_payment_batch(
        &self,
        payments: Vec<DbPayment>,
        auth: BearerAuthToken,
    ) -> Result<(), BackendApiError>;

    /// POST /node/v1/payments/ids [`GetPaymentsByIds`] -> [`Vec<DbPayment>`]
    ///
    /// Fetch a batch of payments by their [`LxPaymentId`]s. This is typically
    /// used by a mobile client to poll for updates on payments which it
    /// currently has stored locally as "pending"; the intention is to check
    /// if any of these payments have been updated.
    // We use POST because there may be a lot of ids, which might be too large
    // to fit inside query parameters.
    async fn get_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
        auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError>;

    /// GET /node/v1/payments/new [`GetNewPayments`] -> [`Vec<DbPayment>`]
    ///
    /// Sync a batch of new payments to local storage, optionally starting from
    /// a known [`PaymentIndex`] (exclusive). Results are in ascending order, by
    /// `(created_at, payment_id)`. See [`GetNewPayments`] for more info.
    ///
    /// [`PaymentIndex`]: crate::ln::payments::PaymentIndex
    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError>;

    /// GET /node/v1/payments/pending -> [`Vec<DbPayment>`]
    ///
    /// Fetches all pending payments.
    async fn get_pending_payments(
        &self,
        auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError>;

    /// GET /node/v1/payments/final -> [`Vec<LxPaymentId>`]
    ///
    /// Fetches the IDs of all finalized payments.
    async fn get_finalized_payment_ids(
        &self,
        auth: BearerAuthToken,
    ) -> Result<Vec<LxPaymentId>, BackendApiError>;
}

/// Defines the api that the backend exposes to the app (via the gateway).
#[async_trait]
pub trait AppBackendApi {
    /// POST /app/v1/signup [`ed25519::Signed<UserSignupRequest>`] -> [`()`]
    async fn signup(
        &self,
        signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError>;
}

/// The bearer auth API exposed by the backend (sometimes via the gateway) to
/// various consumers. This trait is defined separately from the
/// usual `ConsumerServiceApi` traits because [`BearerAuthenticator`] needs to
/// abstract over a generic implementor of [`BearerAuthBackendApi`].
///
/// [`BearerAuthenticator`]: crate::api::auth::BearerAuthenticator
#[async_trait]
pub trait BearerAuthBackendApi {
    /// POST /CONSUMER/bearer_auth [`ed25519::Signed<BearerAuthRequest>`]
    ///                            -> [`BearerAuthResponse`]
    ///
    /// Valid values for `CONSUMER` are: "app", "node" and "lsp".
    async fn bearer_auth(
        &self,
        signed_req: ed25519::Signed<BearerAuthRequest>,
    ) -> Result<BearerAuthResponse, BackendApiError>;
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

/// Defines the api that the node exposes to the runner (Lexe)
#[async_trait]
pub trait RunnerNodeApi {
    /// GET /runner/status [`GetByUserPk`] -> "OK"
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn status(&self, user_pk: UserPk) -> Result<String, NodeApiError>;

    /// GET /runner/shutdown [`GetByUserPk`] -> [`()`]
    ///
    /// [`GetByUserPk`]: super::qs::GetByUserPk
    async fn shutdown(&self, user_pk: UserPk) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the app during provisioning.
#[async_trait]
pub trait AppNodeProvisionApi {
    /// POST /provision [`NodeProvisionRequest`] -> [`()`]
    async fn provision(
        &self,
        data: NodeProvisionRequest,
    ) -> Result<(), NodeApiError>;
}

/// Defines the api that the node exposes to the app during normal operation.
#[async_trait]
pub trait AppNodeRunApi {
    /// GET /app/node_info [`EmptyData`] -> [`NodeInfo`]
    ///
    /// [`EmptyData`]: super::qs::EmptyData
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError>;

    /// POST /app/create_invoice [`CreateInvoiceRequest`] -> [`LxInvoice`]
    async fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> Result<LxInvoice, NodeApiError>;

    /// POST /app/pay_invoice [`PayInvoiceRequest`] -> [`()`]
    async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> Result<(), NodeApiError>;

    /// POST /app/send_onchain [`SendOnchainRequest`] -> [`LxTxid`]
    ///
    /// Returns the [`LxTxid`] of the newly broadcasted transaction.
    async fn send_onchain(
        &self,
        req: SendOnchainRequest,
    ) -> Result<LxTxid, NodeApiError>;

    /// POST /app/get_new_address [`()`] -> [`Address`]
    ///
    /// Returns a new external address which can be used to receive funds.
    async fn get_new_address(&self) -> Result<Address, NodeApiError>;

    /// POST /v1/payments/ids [`GetPaymentsByIds`] -> [`Vec<DbPayment>`]
    ///
    /// Fetch a batch of payments by their [`LxPaymentId`]s. This is typically
    /// used by a mobile client to poll for updates on payments which it
    /// currently has stored locally as "pending"; the intention is to check
    /// if any of these payments have been updated.
    // We use POST because there may be a lot of ids, which might be too large
    // to fit inside query parameters.
    async fn get_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
    ) -> Result<Vec<BasicPayment>, NodeApiError>;

    /// GET /app/payments/new [`GetNewPayments`] -> [`Vec<BasicPayment>`]
    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<Vec<BasicPayment>, NodeApiError>;

    /// PUT /app/payments/note [`UpdatePaymentNote`] -> [`()`]
    async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> Result<(), NodeApiError>;
}

/// Defines the api that the gateway directly exposes to the app.
#[async_trait]
pub trait AppGatewayApi {
    /// GET /app/v1/fiat_rates [`()`] -> [`FiatRates`]
    async fn get_fiat_rates(&self) -> Result<FiatRates, GatewayApiError>;
}
