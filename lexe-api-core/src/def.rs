//! # API Definitions
//!
//! This module, as closely as possible, defines the various APIs exposed by
//! different services to different clients. Although we do not have
//! compile-time guarantees that the services exposed exactly match the
//! definitions below, it is straightforward to compare the Axum routers and
//! handlers with the definitions below to ensure consistency.
//!
//! ## Guidelines
//!
//! All API requests and responses should return structs for upgradeability,
//! e.g. [`UserPkStruct`] instead of [`UserPk`].
//!
//! If an API method takes or returns nothing, make the type [`Empty`] and NOT
//! `()` (unit type). Using `()` makes it impossible to add optional fields in a
//! backwards-compatible way.
//!
//! Each endpoint should be documented with:
//! - 1) HTTP method e.g. `GET`
//! - 2) Endpoint e.g. `/v1/file`
//! - 3) Data used to make the request e.g. `VfsFileId`
//! - 4) The return type e.g. `MaybeVfsFile`
//!
//! The methods below should resemble the data actually sent across the wire.
//!
//! [`Empty`]: crate::types::Empty
//! [`UserPk`]: common::api::user::UserPk
//! [`UserPkStruct`]: common::api::user::UserPkStruct

#![deny(missing_docs)]
// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

use async_trait::async_trait;
use bytes::Bytes;
#[cfg(doc)]
use common::{
    api::user::NodePkStruct, api::user::UserPkStruct,
    api::version::MeasurementStruct, api::MegaIdStruct,
};
use common::{
    api::{
        auth::{
            BearerAuthRequestWire, BearerAuthResponse, BearerAuthToken,
            UserSignupRequestWire, UserSignupRequestWireV1,
        },
        fiat_rates::FiatRates,
        models::{
            SignMsgRequest, SignMsgResponse, Status, VerifyMsgRequest,
            VerifyMsgResponse,
        },
        provision::NodeProvisionRequest,
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            GetRevocableClients, RevocableClients, UpdateClientRequest,
            UpdateClientResponse,
        },
        test_event::TestEventOp,
        user::{
            GetNewScidsRequest, MaybeScid, MaybeUser, NodePk, ScidStruct,
            Scids, UserPk,
        },
        version::NodeRelease,
        MegaId,
    },
    ed25519,
    enclave::Measurement,
};
use lightning::events::Event;

#[cfg(doc)]
use crate::types::payments::PaymentIndex;
use crate::{
    error::{
        BackendApiError, GatewayApiError, LspApiError, MegaApiError,
        NodeApiError, RunnerApiError,
    },
    models::{
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            CreateOfferRequest, CreateOfferResponse, GetAddressResponse,
            GetNewPayments, ListChannelsResponse, NodeInfo, OpenChannelRequest,
            OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
            PayOfferRequest, PayOfferResponse, PayOnchainRequest,
            PayOnchainResponse, PaymentIndexStruct, PaymentIndexes,
            PreflightCloseChannelRequest, PreflightCloseChannelResponse,
            PreflightOpenChannelRequest, PreflightOpenChannelResponse,
            PreflightPayInvoiceRequest, PreflightPayInvoiceResponse,
            PreflightPayOfferRequest, PreflightPayOfferResponse,
            PreflightPayOnchainRequest, PreflightPayOnchainResponse,
            ResyncRequest, UpdatePaymentNote,
        },
        runner::{
            MegaNodeApiUserEvictRequest, MegaNodeApiUserRunRequest,
            MegaNodeApiUserRunResponse, UserFinishedRequest,
            UserLeaseRenewalRequest,
        },
    },
    types::{
        payments::{
            DbPayment, MaybeDbPayment, VecBasicPayment, VecDbPayment,
            VecLxPaymentId,
        },
        ports::MegaPorts,
        sealed_seed::{MaybeSealedSeed, SealedSeed, SealedSeedId},
        Empty,
    },
    vfs::{MaybeVfsFile, VecVfsFile, VfsDirectory, VfsFile, VfsFileId},
};

// TODO(max): To make clear that only upgradeable structs are being serialized,
// these methods should take e.g. `&UserPkStruct` instead of `UserPk`.

/// Defines the api that the backend exposes to the app (via the gateway).
pub trait AppBackendApi {
    /// POST /app/v2/signup [`ed25519::Signed<UserSignupRequestWire>`] ->
    /// [`Empty`]
    async fn signup_v2(
        &self,
        signed_req: &ed25519::Signed<&UserSignupRequestWire>,
    ) -> Result<Empty, BackendApiError>;

    /// POST /app/v1/signup [`ed25519::Signed<UserSignupRequestWireV1>`] ->
    /// [`Empty`]
    // TODO(phlip9): remove once all installed mobile clients above `app-v0.7.6`
    #[deprecated = "Use the `signup_v2` API instead"]
    async fn signup_v1(
        &self,
        signed_req: &ed25519::Signed<&UserSignupRequestWireV1>,
    ) -> Result<Empty, BackendApiError>;
}

/// Defines the api that the gateway directly exposes to the app.
pub trait AppGatewayApi {
    /// GET /app/v1/fiat_rates [`Empty`] -> [`FiatRates`]
    async fn get_fiat_rates(&self) -> Result<FiatRates, GatewayApiError>;

    /// Get the measurement and semver version of the latest node release.
    ///
    /// GET /app/v1/latest_release [`Empty`] -> [`NodeRelease`]
    async fn latest_release(&self) -> Result<NodeRelease, GatewayApiError>;
}

/// Defines the api that the node exposes to the app during provisioning.
pub trait AppNodeProvisionApi {
    /// Provision a node with the given [`Measurement`]. The provisioning node's
    /// remote attestation will be checked against the given [`Measurement`].
    ///
    /// POST /app/provision [`NodeProvisionRequest`] -> [`Empty`]
    async fn provision(
        &self,
        measurement: Measurement,
        data: NodeProvisionRequest,
    ) -> Result<Empty, NodeApiError>;
}

/// Defines the api that the node exposes to the app during normal operation.
pub trait AppNodeRunApi {
    /// GET /app/node_info [`Empty`] -> [`NodeInfo`]
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError>;

    /// GET /app/list_channels [`Empty`] -> [`ListChannelsResponse`]
    async fn list_channels(&self)
        -> Result<ListChannelsResponse, NodeApiError>;

    /// POST /app/sign_message [`SignMsgRequest`] -> [`SignMsgResponse`]
    ///
    /// Introduced in `node-v0.6.5`.
    async fn sign_message(
        &self,
        req: SignMsgRequest,
    ) -> Result<SignMsgResponse, NodeApiError>;

    /// POST /app/verify_message [`VerifyMsgRequest`] -> [`VerifyMsgResponse`]
    ///
    /// Introduced in `node-v0.6.5`.
    async fn verify_message(
        &self,
        req: VerifyMsgRequest,
    ) -> Result<VerifyMsgResponse, NodeApiError>;

    /// POST /app/open_channel [`OpenChannelRequest`] -> [`OpenChannelResponse`]
    ///
    /// Opens a channel to the LSP.
    async fn open_channel(
        &self,
        req: OpenChannelRequest,
    ) -> Result<OpenChannelResponse, NodeApiError>;

    /// POST /app/preflight_open_channel [`PreflightOpenChannelRequest`]
    ///                                  -> [`PreflightOpenChannelResponse`]
    ///
    /// Estimate on-chain fees required for an [`open_channel`] to the LSP.
    ///
    /// [`open_channel`]: AppNodeRunApi::open_channel
    async fn preflight_open_channel(
        &self,
        req: PreflightOpenChannelRequest,
    ) -> Result<PreflightOpenChannelResponse, NodeApiError>;

    /// POST /app/close_channel [`CloseChannelRequest`] -> [`Empty`]
    ///
    /// Closes a channel to the LSP.
    async fn close_channel(
        &self,
        req: CloseChannelRequest,
    ) -> Result<Empty, NodeApiError>;

    /// POST /app/preflight_close_channel [`PreflightCloseChannelRequest`]
    ///                                   -> [`PreflightCloseChannelResponse`]
    ///
    /// Estimate the on-chain fees required for a [`close_channel`].
    ///
    /// [`close_channel`]: AppNodeRunApi::close_channel
    async fn preflight_close_channel(
        &self,
        req: PreflightCloseChannelRequest,
    ) -> Result<PreflightCloseChannelResponse, NodeApiError>;

    /// POST /app/create_invoice [`CreateInvoiceRequest`]
    ///                          -> [`CreateInvoiceResponse`]
    async fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, NodeApiError>;

    /// POST /app/pay_invoice [`PayInvoiceRequest`] -> [`PayInvoiceResponse`]
    async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> Result<PayInvoiceResponse, NodeApiError>;

    /// POST /app/preflight_pay_invoice [`PreflightPayInvoiceRequest`]
    ///                                 -> [`PreflightPayInvoiceResponse`]
    ///
    /// This endpoint lets the app ask its node to "pre-flight" a BOLT11 invoice
    /// payment without going through with the actual payment. We verify as much
    /// as we can, find a route, and get the fee estimates.
    async fn preflight_pay_invoice(
        &self,
        req: PreflightPayInvoiceRequest,
    ) -> Result<PreflightPayInvoiceResponse, NodeApiError>;

    /// POST /app/create_offer [`CreateOfferRequest`] -> [`CreateOfferResponse`]
    ///
    /// Create a new Lightning offer (BOLT12).
    //
    // Added in `node-v0.7.3`.
    async fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> Result<CreateOfferResponse, NodeApiError>;

    /// POST /app/pay_offer [`PayOfferRequest`] -> [`PayOfferResponse`]
    ///
    /// Pay a Lightning offer (BOLT12).
    //
    // Added in `node-v0.7.4`.
    async fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> Result<PayOfferResponse, NodeApiError>;

    /// POST /app/preflight_pay_offer [`PreflightPayOfferRequest`]
    ///                               -> [`PreflightPayOfferResponse`]
    ///
    /// This endpoint lets the app ask its node to "pre-flight" a Lightning
    /// offer (BOLT12) payment without going through with the actual payment. We
    /// verify as much as we can, find a route, and get the fee estimates.
    //
    // Added in `node-v0.7.4`.
    async fn preflight_pay_offer(
        &self,
        req: PreflightPayOfferRequest,
    ) -> Result<PreflightPayOfferResponse, NodeApiError>;

    // TODO(phlip9): BOLT12: /app/request_refund

    /// POST /app/get_address [`Empty`] -> [`GetAddressResponse`]
    ///
    /// Returns an address which can be used to receive funds. It is unused
    /// unless there is an incoming tx and BDK hasn't detected it yet.
    async fn get_address(&self) -> Result<GetAddressResponse, NodeApiError>;

    /// POST /app/pay_onchain [`PayOnchainRequest`] -> [`PayOnchainResponse`]
    ///
    /// Pay bitcoin onchain. If the address is valid and we have sufficient
    /// onchain funds, this will broadcast a new transaction to the bitcoin
    /// mempool.
    async fn pay_onchain(
        &self,
        req: PayOnchainRequest,
    ) -> Result<PayOnchainResponse, NodeApiError>;

    /// POST /app/preflight_pay_onchain [`PreflightPayOnchainRequest`]
    ///                              -> [`PreflightPayOnchainResponse`]
    ///
    /// Returns estimated network fees for a potential onchain payment.
    async fn preflight_pay_onchain(
        &self,
        req: PreflightPayOnchainRequest,
    ) -> Result<PreflightPayOnchainResponse, NodeApiError>;

    /// POST /app/payments/indexes [`PaymentIndexes`] -> [`VecDbPayment`]
    ///
    /// Fetch a batch of payments by their [`PaymentIndex`]s. This is typically
    /// used by a mobile client to poll for updates on payments which it
    /// currently has stored locally as "pending"; the intention is to check
    /// if any of these payments have been updated.
    //
    // We use POST because there may be a lot of idxs, which might be too large
    // to fit inside query parameters.
    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
    ) -> Result<VecBasicPayment, NodeApiError>;

    /// GET /app/payments/new [`GetNewPayments`] -> [`VecBasicPayment`]
    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<VecBasicPayment, NodeApiError>;

    /// PUT /app/payments/note [`UpdatePaymentNote`] -> [`Empty`]
    async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> Result<Empty, NodeApiError>;

    /// Lists all revocable clients.
    ///
    /// GET /app/clients [`GetRevocableClients`] -> [`RevocableClients`]
    // Added in `node-0.7.9`
    async fn get_revocable_clients(
        &self,
        req: GetRevocableClients,
    ) -> Result<RevocableClients, NodeApiError>;

    /// Creates a new revocable client. Returns the newly issued client cert.
    ///
    /// POST /app/clients [`CreateRevocableClientRequest`]
    ///                   -> [`CreateRevocableClientResponse`]
    // Added in `node-0.7.9`
    async fn create_revocable_client(
        &self,
        req: CreateRevocableClientRequest,
    ) -> Result<CreateRevocableClientResponse, NodeApiError>;

    /// Updates this revocable client. Returns the updated client.
    ///
    /// PUT /app/clients [`UpdateClientRequest`] -> [`UpdateClientResponse`]
    // Added in `node-0.7.9`
    async fn update_revocable_client(
        &self,
        req: UpdateClientRequest,
    ) -> Result<UpdateClientResponse, NodeApiError>;
}

/// The bearer auth API exposed by the backend (sometimes via the gateway) to
/// various consumers. This trait is defined separately from the
/// usual `ConsumerServiceApi` traits because `BearerAuthenticator` needs to
/// abstract over a generic implementor of [`BearerAuthBackendApi`].
#[async_trait]
pub trait BearerAuthBackendApi {
    /// POST /CONSUMER/bearer_auth [`ed25519::Signed<BearerAuthRequest>`]
    ///                         -> [`BearerAuthResponse`]
    ///
    /// Valid values for `CONSUMER` are: "app", "node" and "lsp".
    async fn bearer_auth(
        &self,
        signed_req: &ed25519::Signed<&BearerAuthRequestWire>,
    ) -> Result<BearerAuthResponse, BackendApiError>;
}

/// Defines the API the mega node exposes to the Lexe operators.
///
/// NOTE: For performance, this API does not use TLS! This API should only
/// contain methods for limited operational and lifecycle management endpoints.
pub trait LexeMegaApi {
    /// POST /lexe/run_user [`MegaNodeApiUserRunRequest`]
    ///                  -> [`MegaNodeApiUserRunResponse`]
    async fn run_user(
        &self,
        req: MegaNodeApiUserRunRequest,
    ) -> Result<MegaNodeApiUserRunResponse, MegaApiError>;

    /// POST /lexe/evict_user [`MegaNodeApiUserEvictRequest`] -> [`Empty`]
    async fn evict_user(
        &self,
        req: MegaNodeApiUserEvictRequest,
    ) -> Result<Empty, MegaApiError>;

    /// GET /lexe/status [`MegaIdStruct`] -> [`Status`]
    async fn status_mega(
        &self,
        mega_id: MegaId,
    ) -> Result<Status, MegaApiError>;

    /// POST /lexe/shutdown [`Empty`] -> [`Empty`]
    async fn shutdown_mega(&self) -> Result<Empty, MegaApiError>;
}

/// Defines the API the node exposes to the Lexe operators at run time.
///
/// NOTE: For performance, this API does not use TLS! This API should only
/// contain methods for limited operational and lifecycle management endpoints.
pub trait LexeNodeRunApi {
    /// GET /lexe/status [`UserPkStruct`] -> [`Status`]
    async fn status_run(&self, user_pk: UserPk)
        -> Result<Status, NodeApiError>;

    /// POST /lexe/resync [`ResyncRequest`] -> [`Empty`]
    ///
    /// Triggers an immediate resync of BDK and LDK. Optionally full sync the
    /// BDK wallet. Returns only once sync has either completed or timed out.
    async fn resync(&self, req: ResyncRequest) -> Result<Empty, NodeApiError>;

    /// POST /lexe/test_event [`TestEventOp`] -> [`Empty`]
    ///
    /// Calls the corresponding `TestEventReceiver` method.
    /// This endpoint can only be called by one caller at any one time.
    /// Does nothing and returns an error if called in prod.
    async fn test_event(&self, op: &TestEventOp)
        -> Result<Empty, NodeApiError>;

    /// GET /lexe/shutdown [`UserPkStruct`] -> [`Empty`]
    ///
    /// Not to be confused with [`LexeMegaApi::shutdown_mega`].
    async fn shutdown_run(
        &self,
        user_pk: UserPk,
    ) -> Result<Empty, NodeApiError>;
}

/// Defines the API the runner exposes to mega nodes.
#[async_trait] // TODO(max): Remove once we remove `RunnerApiClient`.
pub trait MegaRunnerApi {
    /// POST /mega/ready [`MegaPorts`] -> [`Empty`]
    ///
    /// Indicates this mega node is initialized and ready to load user nodes.
    async fn mega_ready(
        &self,
        ports: &MegaPorts,
    ) -> Result<Empty, RunnerApiError>;

    /// POST /mega/user_finished [`UserFinishedRequest`] -> [`Empty`]
    ///
    /// Notifies the runner that a user has shut down,
    /// and that the user's lease can be terminated.
    async fn user_finished(
        &self,
        req: &UserFinishedRequest,
    ) -> Result<Empty, RunnerApiError>;
}

/// Defines the api that the backend exposes to the node.
#[async_trait]
pub trait NodeBackendApi {
    // --- Unauthenticated --- //

    /// GET /node/v1/user [`UserPkStruct`] -> [`MaybeUser`]
    async fn get_user(
        &self,
        user_pk: UserPk,
    ) -> Result<MaybeUser, BackendApiError>;

    /// GET /node/v1/sealed_seed [`SealedSeedId`] -> [`MaybeSealedSeed`]
    async fn get_sealed_seed(
        &self,
        data: &SealedSeedId,
    ) -> Result<MaybeSealedSeed, BackendApiError>;

    // --- Bearer authentication required --- //

    /// PUT /node/v1/sealed_seed [`SealedSeed`] -> [`Empty`]
    ///
    /// Idempotent: does nothing if the [`SealedSeedId`] already exists.
    async fn create_sealed_seed(
        &self,
        data: &SealedSeed,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// Delete all sealed seeds which have the given measurement and the user_pk
    /// of the authenticated user.
    ///
    /// DELETE /node/v1/sealed_seed [`MeasurementStruct`] -> [`Empty`]
    async fn delete_sealed_seeds(
        &self,
        measurement: Measurement,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// GET /node/v1/scids [`Empty`] -> [`Scids`]
    async fn get_scids(
        &self,
        auth: BearerAuthToken,
    ) -> Result<Scids, BackendApiError>;

    /// GET /node/v1/scid [`Empty`] -> [`MaybeScid`]
    // NOTE: Keep this def around until we can remove the backend handler.
    #[deprecated(note = "since lsp-v0.7.3: Use multi scid version instead")]
    async fn get_scid(
        &self,
        auth: BearerAuthToken,
    ) -> Result<MaybeScid, BackendApiError>;

    /// GET /node/v1/file [`VfsFileId`] -> [`MaybeVfsFile`]
    async fn get_file(
        &self,
        file_id: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<MaybeVfsFile, BackendApiError>;

    /// POST /node/v1/file [`VfsFile`] -> [`Empty`]
    async fn create_file(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// PUT /node/v1/file [`VfsFile`] -> [`Empty`]
    async fn upsert_file(
        &self,
        file: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// DELETE /node/v1/file [`VfsFileId`] -> [`Empty`]
    ///
    /// Returns [`Ok`] only if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// GET /node/v1/directory [`VfsDirectory`] -> [`VecVfsFile`]
    async fn get_directory(
        &self,
        dir: &VfsDirectory,
        auth: BearerAuthToken,
    ) -> Result<VecVfsFile, BackendApiError>;

    /// GET /node/v1/payments [`PaymentIndexStruct`] -> [`MaybeDbPayment`]
    async fn get_payment(
        &self,
        req: PaymentIndexStruct,
        auth: BearerAuthToken,
    ) -> Result<MaybeDbPayment, BackendApiError>;

    /// POST /node/v1/payments [`DbPayment`] -> [`Empty`]
    async fn create_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// PUT /node/v1/payments [`DbPayment`] -> [`Empty`]
    async fn upsert_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// PUT /node/v1/payments/batch [`VecDbPayment`] -> [`Empty`]
    ///
    /// ACID endpoint for upserting a batch of payments.
    async fn upsert_payment_batch(
        &self,
        payments: VecDbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError>;

    /// POST /node/v1/payments/indexes [`PaymentIndexes`]
    ///                             -> [`VecDbPayment`]
    ///
    /// Fetch a batch of payments by their [`PaymentIndex`]s. This is typically
    /// used by a mobile client to poll for updates on payments which it
    /// currently has stored locally as "pending"; the intention is to check
    /// if any of these payments have been updated.
    //
    // We use POST because there may be a lot of idxs, which might be too large
    // to fit inside query parameters.
    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError>;

    /// GET /node/v1/payments/new [`GetNewPayments`] -> [`VecDbPayment`]
    ///
    /// Sync a batch of new payments to local storage, optionally starting from
    /// a known [`PaymentIndex`] (exclusive). Results are in ascending order, by
    /// `(created_at, payment_id)`. See [`GetNewPayments`] for more info.
    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError>;

    /// GET /node/v1/payments/pending -> [`VecDbPayment`]
    ///
    /// Fetches all pending payments.
    async fn get_pending_payments(
        &self,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError>;

    /// GET /node/v1/payments/final -> [`VecLxPaymentId`]
    ///
    /// Fetches the IDs of all finalized payments.
    async fn get_finalized_payment_ids(
        &self,
        auth: BearerAuthToken,
    ) -> Result<VecLxPaymentId, BackendApiError>;
}

/// Defines the api that the LSP exposes to user nodes.
#[async_trait]
pub trait NodeLspApi {
    /// GET /node/v1/scids [`GetNewScidsRequest`] -> [`Scids`]
    async fn get_new_scids(
        &self,
        req: &GetNewScidsRequest,
    ) -> Result<Scids, LspApiError>;

    /// GET /node/v1/scid [`NodePkStruct`] -> [`ScidStruct`]
    // NOTE: Keep this def around until we can remove the LSP handler.
    #[deprecated(note = "since node-v0.7.3: Use multi scid version instead")]
    async fn get_new_scid(
        &self,
        node_pk: NodePk,
    ) -> Result<ScidStruct, LspApiError>;

    /// GET /node/v1/network_graph [`Empty`] -> [`Bytes`] (LDK-serialized graph)
    ///
    /// Introduced in node-v0.6.8 and lsp-v0.6.29.
    async fn get_network_graph(&self) -> Result<Bytes, LspApiError>;

    /// GET /node/v1/prob_scorer [`Empty`]
    ///                       -> [`Bytes`] (LDK-serialized probabilistic scorer)
    ///
    /// Introduced in node-v0.6.17 and lsp-v0.6.33.
    async fn get_prob_scorer(&self) -> Result<Bytes, LspApiError>;

    /// POST /node/v1/payment_path [`Bytes`] (LDK-serialized [`Event`])
    ///                         -> [`Empty`]
    ///
    /// Sends an anonymized successful or failed payment path to the LSP to
    /// update Lexe's shared network graph and improve payment reliability.
    ///
    /// Introduced in node-v0.6.17 and lsp-v0.6.33.
    async fn payment_path(&self, event: &Event) -> Result<Empty, LspApiError>;
}

/// Defines the api that the runner exposes to the node.
#[async_trait]
pub trait NodeRunnerApi {
    /// POST /node/renew_lease [`UserLeaseRenewalRequest`] -> [`Empty`]
    ///
    /// Renew's a user node's lease with the megarunner.
    async fn renew_lease(
        &self,
        req: &UserLeaseRenewalRequest,
    ) -> Result<Empty, RunnerApiError>;

    /// POST /node/activity [`UserPkStruct`] -> [`Empty`]
    ///
    /// Indicates the node received some activity from its user.
    async fn activity(&self, user_pk: UserPk) -> Result<Empty, RunnerApiError>;

    /// POST /node/sync_success [`UserPkStruct`] -> [`Empty`]
    ///
    /// Indicates the node successfully completed sync.
    async fn sync_succ(&self, user_pk: UserPk)
        -> Result<Empty, RunnerApiError>;
}
