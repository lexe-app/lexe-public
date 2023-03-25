use async_trait::async_trait;
use common::api::auth::{
    UserAuthRequest, UserAuthResponse, UserAuthToken, UserSignupRequest,
};
use common::api::def::{
    NodeBackendApi, NodeLspApi, NodeRunnerApi, UserBackendApi,
};
use common::api::error::{BackendApiError, LspApiError, RunnerApiError};
use common::api::ports::UserPorts;
use common::api::provision::{SealedSeed, SealedSeedId};
use common::api::qs::{GetByNodePk, GetByUserPk};
use common::api::rest::{RequestBuilderExt, RestClient, POST};
use common::api::vfs::{NodeDirectory, NodeFile, NodeFileId};
use common::api::{NodePk, Scid, User, UserPk};
use common::ed25519;

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
    async fn ready(
        &self,
        data: UserPorts,
    ) -> Result<UserPorts, RunnerApiError> {
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
        let req = self.rest.get(format!("{lsp}/v1/scid"), &data);
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
        data: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/v1/file");
        let req = self.rest.post(url, data).bearer_auth(&auth);
        self.rest.send_with_retries(req, retries, &[]).await
    }

    async fn upsert_file_with_retries(
        &self,
        data: &NodeFile,
        auth: UserAuthToken,
        retries: usize,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/v1/file");
        let req = self.rest.put(url, data).bearer_auth(&auth);
        self.rest.send_with_retries(req, retries, &[]).await
    }
}

#[async_trait]
impl UserBackendApi for BackendClient {
    async fn signup(
        &self,
        _signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError> {
        unimplemented!()
    }

    async fn user_auth(
        &self,
        signed_req: ed25519::Signed<UserAuthRequest>,
    ) -> Result<UserAuthResponse, BackendApiError> {
        let backend = &self.backend_url;
        let url = format!("{backend}/user_auth");
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
        let req = self.rest.get(format!("{backend}/v1/user"), &data);
        self.rest.send(req).await
    }

    // not authenticated, node calls this to get sealed seed on startup
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError> {
        let backend = &self.backend_url;
        let req = self.rest.get(format!("{backend}/v1/sealed_seed"), &data);
        self.rest.send(req).await
    }

    async fn create_sealed_seed(
        &self,
        data: SealedSeed,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/v1/sealed_seed"), &data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_scid(
        &self,
        node_pk: NodePk,
    ) -> Result<Option<Scid>, BackendApiError> {
        let backend = &self.backend_url;
        let data = GetByNodePk { node_pk };
        let req = self.rest.get(format!("{backend}/v1/scid"), &data);
        self.rest.send(req).await
    }

    async fn get_file(
        &self,
        data: &NodeFileId,
        auth: UserAuthToken,
    ) -> Result<Option<NodeFile>, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn create_file(
        &self,
        data: &NodeFile,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .post(format!("{backend}/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn upsert_file(
        &self,
        data: &NodeFile,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .put(format!("{backend}/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(
        &self,
        data: &NodeFileId,
        auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .delete(format!("{backend}/v1/file"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn get_directory(
        &self,
        data: &NodeDirectory,
        auth: UserAuthToken,
    ) -> Result<Vec<NodeFile>, BackendApiError> {
        let backend = &self.backend_url;
        let req = self
            .rest
            .get(format!("{backend}/v1/directory"), data)
            .bearer_auth(&auth);
        self.rest.send(req).await
    }
}
