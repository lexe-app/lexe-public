use async_trait::async_trait;
use common::{
    api::{
        auth::{BearerAuthRequest, BearerAuthResponse, BearerAuthToken},
        def::{
            BearerAuthBackendApi, NodeBackendApi, NodeLspApi, NodeRunnerApi,
        },
        error::{BackendApiError, LspApiError, RunnerApiError},
        ports::Ports,
        provision::{SealedSeed, SealedSeedId},
        qs::{
            GetByNodePk, GetByUserPk, GetNewPayments, GetPaymentByIndex,
            GetPaymentsByIds,
        },
        rest::{RequestBuilderExt, RestClient, POST},
        vfs::{VfsDirectory, VfsFile, VfsFileId},
        Empty, NodePk, Scid, User, UserPk,
    },
    ed25519,
    ln::payments::{DbPayment, LxPaymentId},
};

use crate::api::BackendApiClient;

pub(crate) struct RunnerClient {
    rest: RestClient,
    runner_url: String,
}

impl RunnerClient {
    #[allow(dead_code)] // TODO(max): Remove
    pub(crate) fn new(runner_url: String) -> Self {
        Self {
            rest: RestClient::new(),
            runner_url,
        }
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
    #[allow(dead_code)] // TODO(max): Remove
    pub(crate) fn new(lsp_url: String) -> Self {
        Self {
            rest: RestClient::new(),
            lsp_url,
        }
    }
}

#[async_trait]
impl NodeLspApi for LspClient {
    async fn get_new_scid(&self, node_pk: NodePk) -> Result<Scid, LspApiError> {
        let lsp = &self.lsp_url;
        let data = GetByNodePk { node_pk };
        let req = self.rest.get(format!("{lsp}/node/v1/scid"), &data);
        self.rest.send(req).await
    }
}

pub(crate) struct BackendClient {
    rest: RestClient,
    backend_url: String,
}

impl BackendClient {
    #[allow(dead_code)] // TODO(max): Remove
    pub(crate) fn new(backend_url: String) -> Self {
        Self {
            rest: RestClient::new(),
            backend_url,
        }
    }
}

#[async_trait]
impl BackendApiClient for BackendClient {
    async fn create_file_with_retries(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
        retries: usize,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/node/v1/file");
        let req = self.rest.post(url, data).bearer_auth(&auth);
        self.rest.send_with_retries(req, retries, &[]).await
    }

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
        signed_req: ed25519::Signed<BearerAuthRequest>,
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
    ) -> Result<Option<User>, BackendApiError> {
        let backend = &self.backend_url;
        let data = GetByUserPk { user_pk };
        let req = self.rest.get(format!("{backend}/node/v1/user"), &data);
        self.rest.send(req).await
    }

    // not authenticated, node calls this to get sealed seed on startup
    async fn get_sealed_seed(
        &self,
        data: &SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError> {
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

    async fn get_scid(
        &self,
        node_pk: NodePk,
        auth: BearerAuthToken,
    ) -> Result<Option<Scid>, BackendApiError> {
        let backend = &self.backend_url;
        let data = GetByNodePk { node_pk };
        let req = self
            .rest
            .get(format!("{backend}/node/v1/scid"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_file(
        &self,
        data: &VfsFileId,
        auth: BearerAuthToken,
    ) -> Result<Option<VfsFile>, BackendApiError> {
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

    // TODO We want to delete channel peers / monitors when channels close
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
    ) -> Result<Vec<VfsFile>, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/node/v1/directory"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payment(
        &self,
        req: GetPaymentByIndex,
        auth: BearerAuthToken,
    ) -> Result<Option<DbPayment>, BackendApiError> {
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
        payments: Vec<DbPayment>,
        auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/node/v1/payments/batch"), &payments)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
        auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
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
    ) -> Result<Vec<DbPayment>, BackendApiError> {
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
    ) -> Result<Vec<DbPayment>, BackendApiError> {
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
    ) -> Result<Vec<LxPaymentId>, BackendApiError> {
        let backend = &self.backend_url;
        let data = Empty {};
        let req = self
            .rest
            .get(format!("{backend}/node/v1/payments/final"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }
}
