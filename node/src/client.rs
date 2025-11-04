use std::collections::HashSet;

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use common::{
    api::{
        auth::{BearerAuthRequestWire, BearerAuthResponse, BearerAuthToken},
        user::{
            GetNewScidsRequest, MaybeScid, MaybeUser, Scids, UserPk, UserPkSet,
            UserPkStruct,
        },
        version::MeasurementStruct,
    },
    ed25519,
    enclave::Measurement,
    env::DeployEnv,
    rng::Crng,
};
use lexe_api::{
    def::{
        BearerAuthBackendApi, MegaRunnerApi, NodeBackendApi, NodeLspApi,
        NodeRunnerApi,
    },
    error::{BackendApiError, LspApiError, RunnerApiError},
    models::{
        command::{
            GetNewPayments, GetUpdatedPaymentMetadata, GetUpdatedPayments,
            PaymentAddress, PaymentCreatedIndexStruct, PaymentCreatedIndexes,
            PaymentIdStruct, UpdatePaymentAddress, VecLxPaymentId,
        },
        runner::{UserFinishedRequest, UserLeaseRenewalRequest},
    },
    rest::{POST, PUT, RequestBuilderExt, RestClient},
    types::{
        Empty,
        payments::{
            DbPaymentV1, DbPaymentV2, MaybeDbPaymentV1, VecDbPaymentMetadata,
            VecDbPaymentV1, VecDbPaymentV2,
        },
        ports::MegaPorts,
        sealed_seed::{MaybeSealedSeed, SealedSeed, SealedSeedId},
    },
    vfs::{
        MaybeVfsFile, VecVfsFile, VfsDirectory, VfsDirectoryList, VfsFile,
        VfsFileId,
    },
};
use lexe_tls::attestation::{self, NodeMode};
use lightning::events::Event;

/// The user agent string for external requests.
pub static USER_AGENT_EXTERNAL: &str = lexe_api::user_agent_external!();

/// Used for both [`MegaRunnerApi`] and [`NodeRunnerApi`].
pub(crate) struct RunnerClient {
    rest: RestClient,
    runner_url: String,
}

impl RunnerClient {
    pub(crate) fn new(
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
        node_mode: NodeMode,
        runner_url: String,
    ) -> anyhow::Result<Self> {
        let tls_config =
            attestation::node_lexe_client_config(rng, deploy_env, node_mode)
                .context("Failed to build Node->Lexe client TLS config")?;
        let rest = RestClient::new("node", "runner", tls_config);
        Ok(Self { rest, runner_url })
    }
}

impl MegaRunnerApi for RunnerClient {
    async fn mega_ready(
        &self,
        ports: &MegaPorts,
    ) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let req = self.rest.post(format!("{runner}/mega/ready"), &ports);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }

    async fn activity(
        &self,
        user_pks: HashSet<UserPk>,
    ) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let data = UserPkSet { user_pks };
        let req = self.rest.post(format!("{runner}/mega/activity"), &data);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }

    async fn user_finished(
        &self,
        req: &UserFinishedRequest,
    ) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let req = self.rest.post(format!("{runner}/mega/user_finished"), req);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }
}

impl NodeRunnerApi for RunnerClient {
    async fn renew_lease(
        &self,
        req: &UserLeaseRenewalRequest,
    ) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let req = self.rest.post(format!("{runner}/node/renew_lease"), req);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }

    async fn sync_succ(
        &self,
        user_pk: UserPk,
    ) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let data = UserPkStruct { user_pk };
        let req = self.rest.post(format!("{runner}/node/sync_success"), &data);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }
}

pub(crate) struct NodeLspClient {
    rest: RestClient,
    lsp_url: String,
}

impl NodeLspClient {
    pub(crate) fn new(
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
        node_mode: NodeMode,
        lsp_url: String,
    ) -> anyhow::Result<Self> {
        let tls_config =
            attestation::node_lexe_client_config(rng, deploy_env, node_mode)
                .context("Failed to build Node->Lexe client TLS config")?;
        let rest = RestClient::new("node", "lsp", tls_config);

        Ok(Self { rest, lsp_url })
    }
}

impl NodeLspApi for NodeLspClient {
    async fn get_new_scids(
        &self,
        req: &GetNewScidsRequest,
    ) -> Result<Scids, LspApiError> {
        let lsp = &self.lsp_url;
        let req = self.rest.get(format!("{lsp}/node/v1/scids"), &req);
        self.rest.send(req).await
    }

    async fn get_network_graph(&self) -> Result<Bytes, LspApiError> {
        let lsp = &self.lsp_url;
        let data = Empty {};
        let req = self.rest.get(format!("{lsp}/node/v1/network_graph"), &data);
        self.rest.send_no_deserialize::<LspApiError>(req).await
    }

    async fn get_prob_scorer(&self) -> Result<Bytes, LspApiError> {
        let lsp = &self.lsp_url;
        let data = Empty {};
        let req = self.rest.get(format!("{lsp}/node/v1/prob_scorer"), &data);
        self.rest.send_no_deserialize::<LspApiError>(req).await
    }

    async fn payment_path(&self, event: &Event) -> Result<Empty, LspApiError> {
        let lsp = &self.lsp_url;
        let url = format!("{lsp}/node/v1/payment_path");
        let req = self.rest.serialize_ldk_writeable(POST, url, event);
        self.rest.send(req).await
    }
}

pub(crate) struct NodeBackendClient {
    rest: RestClient,
    backend_url: String,
}

impl NodeBackendClient {
    pub(crate) fn new(
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
        node_mode: NodeMode,
        backend_url: String,
    ) -> anyhow::Result<Self> {
        let tls_config =
            attestation::node_lexe_client_config(rng, deploy_env, node_mode)
                .context("Failed to build Node->Lexe client TLS config")?;

        let rest = RestClient::new("node", "backend", tls_config);

        Ok(Self { rest, backend_url })
    }
}

impl NodeBackendClient {
    pub(crate) async fn upsert_file_with_retries(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        auth: BearerAuthToken,
        retries: usize,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .builder(PUT, format!("{backend}/node/v2/file"))
            .query(file_id)
            .body(data)
            .bearer_auth(&auth);
        self.rest.send_with_retries(req, retries, &[]).await
    }
}

#[async_trait]
impl BearerAuthBackendApi for NodeBackendClient {
    async fn bearer_auth(
        &self,
        signed_req: &ed25519::Signed<&BearerAuthRequestWire>,
    ) -> Result<BearerAuthResponse, BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/node/bearer_auth");
        let req = self
            .rest
            .builder(POST, url)
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send_with_retries(req, 3, &[]).await
    }
}

impl NodeBackendApi for NodeBackendClient {
    // not authenticated, node calls this to get sealed seed on startup
    async fn get_user(
        &self,
        user_pk: UserPk,
    ) -> Result<MaybeUser, BackendApiError> {
        let backend = &self.backend_url;
        let data = UserPkStruct { user_pk };
        let req = self.rest.get(format!("{backend}/node/v1/user"), &data);
        self.rest.send(req).await
    }

    // not authenticated, node calls this to get sealed seed on startup
    async fn get_sealed_seed(
        &self,
        data: &SealedSeedId,
    ) -> Result<MaybeSealedSeed, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/sealed_seed"), data);
        self.rest.send(req).await
    }

    async fn create_sealed_seed(
        &self,
        data: &SealedSeed,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/sealed_seed"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn delete_sealed_seeds(
        &self,
        measurement: Measurement,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let data = MeasurementStruct { measurement };
        let req = self
            .rest
            .delete(format!("{backend}/node/v1/sealed_seed"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_scids(
        &self,
        auth: BearerAuthToken,
    ) -> Result<Scids, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/scids"), &Empty {})
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_scid(
        &self,
        auth: BearerAuthToken,
    ) -> Result<MaybeScid, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/scid"), &Empty {})
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_file_v1(
        &self,
        data: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<MaybeVfsFile, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_file(
        &self,
        file_id: &VfsFileId,
        token: BearerAuthToken,
    ) -> Result<bytes::Bytes, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v2/file"), file_id)
            .bearer_auth(&token);
        self.rest.send_no_deserialize(req).await
    }

    async fn create_file_v1(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn create_file(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .builder(POST, format!("{backend}/node/v2/file"))
            .query(file_id)
            .body(data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_file_v1(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_file(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .builder(PUT, format!("{backend}/node/v2/file"))
            .query(file_id)
            .body(data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    // TODO We want to delete LN peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(
        &self,
        data: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .delete(format!("{backend}/node/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_directory_v1(
        &self,
        data: &VfsDirectory,
        auth: BearerAuthToken,
    ) -> Result<VecVfsFile, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/directory"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn list_directory(
        &self,
        dir: &VfsDirectory,
        auth: BearerAuthToken,
    ) -> Result<VfsDirectoryList, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v2/directory"), dir)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment_by_index_v1(
        &self,
        req: PaymentCreatedIndexStruct,
        auth: BearerAuthToken,
    ) -> Result<MaybeDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn create_payment(
        &self,
        payment: DbPaymentV1,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments"), &payment)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment_v1(
        &self,
        payment: DbPaymentV1,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payments"), &payment)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment(
        &self,
        payment: DbPaymentV2,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v2/payments"), &payment)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment_by_id(
        &self,
        req: PaymentIdStruct,
        auth: BearerAuthToken,
    ) -> Result<MaybeDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/id"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment_by_index(
        &self,
        req: PaymentCreatedIndexStruct,
        auth: BearerAuthToken,
    ) -> Result<MaybeDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/index"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment_batch_v1(
        &self,
        payments: VecDbPaymentV1,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payments/batch"), &payments)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment_batch(
        &self,
        payments: VecDbPaymentV2,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v2/payments/batch"), &payments)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payments_by_indexes(
        &self,
        req: PaymentCreatedIndexes,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments/indexes"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payments_by_ids(
        &self,
        req: VecLxPaymentId,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentV2, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments/ids"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/new"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_updated_payments(
        &self,
        req: GetUpdatedPayments,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentV2, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/updated"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_pending_payments(
        &self,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentV1, BackendApiError> {
        let backend = &self.backend_url;
        let data = Empty {};
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/pending"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_finalized_payment_ids(
        &self,
        auth: BearerAuthToken,
    ) -> Result<VecLxPaymentId, BackendApiError> {
        let backend = &self.backend_url;
        let data = Empty {};
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/final"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment_metadata_by_ids(
        &self,
        req: VecLxPaymentId,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentMetadata, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments/metadata/ids"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_updated_payment_metadata(
        &self,
        req: GetUpdatedPaymentMetadata,
        auth: BearerAuthToken,
    ) -> Result<VecDbPaymentMetadata, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/metadata/updated"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn update_payment_address(
        &self,
        req: UpdatePaymentAddress,
        auth: BearerAuthToken,
    ) -> Result<PaymentAddress, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payment_address"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment_address(
        &self,
        auth: BearerAuthToken,
    ) -> Result<PaymentAddress, BackendApiError> {
        let backend = &self.backend_url;
        let data = Empty {};
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payment_address"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }
}
