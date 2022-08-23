//! This file contains LexeApiClient, the concrete impl of the ApiClient trait.

use std::fmt::{self, Display};

use async_trait::async_trait;
use common::api::def::{NodeBackendApi, NodeRunnerApi};
use common::api::error::{BackendApiError, RunnerApiError};
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use common::api::qs::{GetByUserPk, GetByUserPkAndMeasurement};
use common::api::rest::{RestClient, DELETE, GET, POST, PUT};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::Measurement;

use self::ApiVersion::*;
use self::BaseUrl::*;
use crate::api::ApiClient;

/// Enumerates the base urls that can be used in an API call.
#[derive(Copy, Clone)]
enum BaseUrl {
    Backend,
    Runner,
}

#[derive(Copy, Clone)]
enum ApiVersion {
    V1,
}

impl Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            &V1 => write!(f, "/v1"),
        }
    }
}

pub struct LexeApiClient {
    rest: RestClient,
    backend_url: String,
    runner_url: String,
}

impl LexeApiClient {
    pub fn new(backend_url: String, runner_url: String) -> Self {
        let rest = RestClient::new();
        Self {
            rest,
            backend_url,
            runner_url,
        }
    }
}

#[async_trait]
impl ApiClient for LexeApiClient {
    async fn create_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest
            .request_with_retries(POST, url, &data, retries)
            .await
    }

    async fn upsert_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest
            .request_with_retries(PUT, url, &data, retries)
            .await
    }
}

#[async_trait]
impl NodeBackendApi for LexeApiClient {
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, BackendApiError> {
        let data = GetByUserPk { user_pk };
        let url = self.build_url(Backend, V1, "/node");
        self.rest.request(GET, url, &data).await
    }

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, BackendApiError> {
        let data = GetByUserPkAndMeasurement {
            user_pk,
            measurement,
        };

        let url = self.build_url(Backend, V1, "/instance");
        self.rest.request(GET, url, &data).await
    }

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError> {
        let url = self.build_url(Backend, V1, "/sealed_seed");
        self.rest.request(GET, url, &data).await
    }

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, BackendApiError> {
        let url = self.build_url(Backend, V1, "/acid/node_instance_seed");
        self.rest.request(POST, url, &data).await
    }

    async fn get_file(
        &self,
        data: &FileId,
    ) -> Result<Option<File>, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest.request(GET, url, &data).await
    }

    async fn create_file(&self, data: &File) -> Result<File, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest.request(POST, url, &data).await
    }

    async fn upsert_file(&self, data: &File) -> Result<File, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest.request(PUT, url, &data).await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(
        &self,
        data: &FileId,
    ) -> Result<String, BackendApiError> {
        let url = self.build_url(Backend, V1, "/file");
        self.rest.request(DELETE, url, &data).await
    }

    async fn get_directory(
        &self,
        data: &Directory,
    ) -> Result<Vec<File>, BackendApiError> {
        let url = self.build_url(Backend, V1, "/directory");
        self.rest.request(GET, url, &data).await
    }
}

#[async_trait]
impl NodeRunnerApi for LexeApiClient {
    async fn notify_runner(
        &self,
        data: UserPorts,
    ) -> Result<UserPorts, RunnerApiError> {
        let url = self.build_url(Runner, V1, "/ready");
        self.rest.request(POST, url, &data).await
    }
}

impl LexeApiClient {
    /// Constructs the request URL including the base, version, and endpoint
    /// (NOT including the query string)
    fn build_url(
        &self,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
    ) -> String {
        // Backend api is versioned but runner api is not
        let (base, ver) = match base {
            Backend => (&self.backend_url, ver.to_string()),
            Runner => (&self.runner_url, String::new()),
        };
        format!("{}{}{}", base, ver, endpoint)
    }
}
