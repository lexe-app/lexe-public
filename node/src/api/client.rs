//! This file contains LexeApiClient, the concrete impl of the ApiClient trait.

use std::cmp::min;
use std::fmt::{self, Display};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use common::api::rest::RestClient;
use common::api::rest::RequestParts;
use common::api::rest::{GET, PUT, POST, DELETE};
use common::api::provision::SealedSeedId;
use common::api::qs::{GetByUserPk, GetByUserPkAndMeasurement};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::Measurement;
use http::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::time;
use tracing::{debug, trace, warn};

use self::ApiVersion::*;
use self::BaseUrl::*;
use crate::api::*;

const DEFAULT_RETRIES: usize = 0;

// Exponential backup
const INITIAL_WAIT_MS: u64 = 250;
const MAXIMUM_WAIT_MS: u64 = 32_000;
const EXP_BASE: u64 = 2;

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
impl BackendService for LexeApiClient {
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, RestError> {
        let data = GetByUserPk { user_pk };
        self.request(GET, Backend, V1, "/node", &data).await
    }

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, RestError> {
        let data = GetByUserPkAndMeasurement {
            user_pk,
            measurement,
        };
        let maybe_instance: Option<Instance> =
            self.request(GET, Backend, V1, "/instance", &data).await?;

        Ok(maybe_instance)
    }

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, RestError> {
        self.request(GET, Backend, V1, "/sealed_seed", &data).await
    }

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, RestError> {
        let endpoint = "/acid/node_instance_seed";
        self.request(POST, Backend, V1, endpoint, &data).await
    }

    async fn get_file(&self, data: &FileId) -> Result<Option<File>, RestError> {
        let endpoint = "/file";
        self.request(GET, Backend, V1, endpoint, &data).await
    }

    async fn create_file(&self, data: &File) -> Result<File, RestError> {
        let endpoint = "/file";
        self.request(POST, Backend, V1, endpoint, &data).await
    }

    // TODO(max): Remove from service definition
    async fn create_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, RestError> {
        let endpoint = "/file";
        self.request_with_retries(POST, Backend, V1, endpoint, &data, retries)
            .await
    }

    async fn upsert_file(&self, data: &File) -> Result<File, RestError> {
        let endpoint = "/file";
        self.request(PUT, Backend, V1, endpoint, &data).await
    }

    // TODO(max): Remove from service definition
    async fn upsert_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, RestError> {
        let endpoint = "/file";
        self.request_with_retries(PUT, Backend, V1, endpoint, &data, retries)
            .await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(&self, data: &FileId) -> Result<String, RestError> {
        let endpoint = "/file";
        self.request(DELETE, Backend, V1, endpoint, &data).await
    }

    async fn get_directory(
        &self,
        data: &Directory,
    ) -> Result<Vec<File>, RestError> {
        let endpoint = "/directory";
        self.request(GET, Backend, V1, endpoint, &data).await
    }
}

#[async_trait]
impl RunnerService for LexeApiClient {
    async fn notify_runner(
        &self,
        data: UserPorts,
    ) -> Result<UserPorts, RestError> {
        self.request(POST, Runner, V1, "/ready", &data).await
    }
}

impl LexeApiClient {
    /// Makes an API request, retrying up to `DEFAULT_RETRIES` times.
    async fn request<D: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
    ) -> Result<T, RestError> {
        self.request_with_retries(
            method,
            base,
            ver,
            endpoint,
            data,
            DEFAULT_RETRIES,
        )
        .await
    }

    /// Makes an API request, retrying up to `retries` times.
    async fn request_with_retries<D: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
        retries: usize,
    ) -> Result<T, RestError> {
        // Serialize request parts
        let parts = self.serialize_parts(method, base, ver, endpoint, data)?;

        // Exponential backup
        let mut backup_durations = (0..)
            .map(|index| INITIAL_WAIT_MS * EXP_BASE.pow(index))
            .map(|wait| min(wait, MAXIMUM_WAIT_MS))
            .map(Duration::from_millis);

        // Do the 'retries' first and return early if successful.
        // This block is a noop if retries == 0.
        for _ in 0..retries {
            let res = self.rest.send_request(&parts).await;
            if res.is_ok() {
                return res;
            } else {
                let method = &parts.method;
                let url = &parts.url;
                warn!("{method} {url} failed.");

                time::sleep(backup_durations.next().unwrap()).await;
            }
        }

        // Do the 'main' attempt.
        self.rest.send_request(&parts).await
    }

    /// Constructs the final, serialized parts of a [`reqwest::Request`].
    fn serialize_parts<D: Serialize>(
        &self,
        method: Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
    ) -> Result<RequestParts, RestError> {
        // Node backend api is versioned but runner api is not
        let (base, ver) = match base {
            Backend => (&self.backend_url, ver.to_string()),
            Runner => (&self.runner_url, String::new()),
        };
        let mut url = format!("{}{}{}", base, ver, endpoint);

        // If GET, serialize the data in a query string
        let query_str = match method {
            GET => Some(serde_qs::to_string(data)?),
            _ => None,
        };
        // Append directly to url since RequestBuilder.param() API is unwieldy
        if let Some(query_str) = query_str {
            if !query_str.is_empty() {
                url.push('?');
                url.push_str(&query_str);
            }
        }
        debug!(%method, %url, "sending request");

        // If PUT or POST, serialize the data in the request body
        let body_str = match method {
            PUT | POST => serde_json::to_string(data)?,
            _ => String::new(),
        };
        trace!(%body_str);
        let body = Bytes::from(body_str);

        Ok(RequestParts { method, url, body })
    }

}
