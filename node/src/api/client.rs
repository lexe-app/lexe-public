use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use common::{
    api::{
        auth::{BearerAuthRequest, BearerAuthResponse, BearerAuthToken},
        command::{GetNewPayments, PaymentIndexStruct, PaymentIndexes},
        def::{
            BearerAuthBackendApi, NodeBackendApi, NodeLspApi, NodeRunnerApi,
        },
        error::{BackendApiError, LspApiError, RunnerApiError},
        ports::Ports,
        provision::{MaybeSealedSeed, SealedSeed, SealedSeedId},
        user::{
            MaybeScid, MaybeUser, NodePk, NodePkStruct, ScidStruct, UserPk,
            UserPkStruct,
        },
        version::MeasurementStruct,
        vfs::{MaybeVfsFile, VecVfsFile, VfsDirectory, VfsFile, VfsFileId},
        Empty,
    },
    ed25519,
    enclave::Measurement,
    env::DeployEnv,
    ln::payments::{DbPayment, MaybeDbPayment, VecDbPayment, VecLxPaymentId},
    rng::Crng,
};
use lexe_api::{
    rest::{RequestBuilderExt, RestClient, POST},
    tls::attestation::{self, NodeMode},
};

use crate::api::BackendApiClient;

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

#[async_trait]
impl NodeRunnerApi for RunnerClient {
    async fn ready(&self, data: &Ports) -> Result<Empty, RunnerApiError> {
        let runner = &self.runner_url;
        let req = self.rest.post(format!("{runner}/node/ready"), &data);
        // TODO(phlip9): authenticate runner callbacks?
        // .bearer_auth(&self.auth_token().await?);
        self.rest.send(req).await
    }
}

pub(crate) struct LspClient {
    rest: RestClient,
    lsp_url: String,
}

impl LspClient {
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

#[async_trait]
impl NodeLspApi for LspClient {
    async fn get_new_scid(
        &self,
        node_pk: NodePk,
    ) -> Result<ScidStruct, LspApiError> {
        let lsp = &self.lsp_url;
        let data = NodePkStruct { node_pk };
        let req = self.rest.get(format!("{lsp}/node/v1/scid"), &data);
        self.rest.send(req).await
    }

    async fn get_network_graph(&self) -> Result<Bytes, LspApiError> {
        let lsp = &self.lsp_url;
        let data = Empty {};
        let req = self.rest.get(format!("{lsp}/node/v1/network_graph"), &data);
        self.rest.send_no_deserialize::<LspApiError>(req).await
    }
}

pub(crate) struct BackendClient {
    rest: RestClient,
    backend_url: String,
}

impl BackendClient {
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

#[async_trait]
impl BackendApiClient for BackendClient {
    async fn upsert_file_with_retries(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
        retries: usize,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/node/v1/file");
        let req = self.rest.put(url, data).bearer_auth(&auth);
        self.rest.send_with_retries(req, retries, &[]).await
    }
}

#[async_trait]
impl BearerAuthBackendApi for BackendClient {
    async fn bearer_auth(
        &self,
        signed_req: &ed25519::Signed<&BearerAuthRequest>,
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

#[async_trait]
impl NodeBackendApi for BackendClient {
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

    async fn get_file(
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

    async fn create_file(
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

    async fn upsert_file(
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

    async fn get_directory(
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

    async fn get_payment(
        &self,
        req: PaymentIndexStruct,
        auth: BearerAuthToken,
    ) -> Result<MaybeDbPayment, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn create_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments"), &payment)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment(
        &self,
        payment: DbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payments"), &payment)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_payment_batch(
        &self,
        payments: VecDbPayment,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payments/batch"), &payments)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/node/v1/payments/indexes"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/new"), &req)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_pending_payments(
        &self,
        auth: BearerAuthToken,
    ) -> Result<VecDbPayment, BackendApiError> {
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
}
