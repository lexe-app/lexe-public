//! This file contains LexeApiClient, the concrete impl of the ApiClient trait.

use std::fmt::{self, Display};
use std::time::Duration;

use async_trait::async_trait;
use common::api::provision::SealedSeedId;
use common::api::qs::{GetByUserPk, GetByUserPkAndMeasurement};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::Measurement;
use http::Method;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::time;
use tracing::debug;

use self::ApiVersion::*;
use self::BaseUrl::*;
use crate::api::*;

const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait after the second failed API request before trying again.
/// This is relatively long since if the second try failed, it probably means
/// that the backend is down, which could be the case for a while.
const RETRY_INTERVAL: Duration = Duration::from_secs(15);

// Avoid `Method::` prefix. Associated constants can't be imported
const GET: Method = Method::GET;
const PUT: Method = Method::PUT;
const POST: Method = Method::POST;
const DELETE: Method = Method::DELETE;

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
    client: Client,
    backend_url: String,
    runner_url: String,
}

impl LexeApiClient {
    pub fn new(backend_url: String, runner_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_REQUEST_TIMEOUT)
            .build()
            .expect("Failed to build reqwest Client");
        Self {
            client,
            backend_url,
            runner_url,
        }
    }
}

#[async_trait]
impl ApiClient for LexeApiClient {
    async fn get_node(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<Node>, ApiError> {
        let data = GetByUserPk { user_pk };
        self.request(&GET, Backend, V1, "/node", &data).await
    }

    async fn get_instance(
        &self,
        user_pk: UserPk,
        measurement: Measurement,
    ) -> Result<Option<Instance>, ApiError> {
        let data = GetByUserPkAndMeasurement {
            user_pk,
            measurement,
        };
        let maybe_instance: Option<Instance> = self
            .request(&GET, Backend, V1, "/instance", &data).await?;

        if let Some(instance) = maybe_instance.as_ref() {
            if instance.measurement != measurement {
                let msg = format!(
                    "returned instance measurement '{}' doesn't match \
                     requested measurement '{}'",
                    instance.measurement, measurement,
                );
                return Err(ApiError::ResponseError(msg));
            }
        }

        Ok(maybe_instance)
    }

    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, ApiError> {
        self.request(&GET, Backend, V1, "/sealed_seed", &data).await
    }

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, ApiError> {
        let endpoint = "/acid/node_instance_seed";
        self.request(&POST, Backend, V1, endpoint, &data).await
    }

    async fn get_file(&self, data: &FileId) -> Result<Option<File>, ApiError> {
        let endpoint = "/file";
        self.request(&GET, Backend, V1, endpoint, &data).await
    }

    async fn create_file(&self, data: &File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(&POST, Backend, V1, endpoint, &data).await
    }

    async fn create_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request_with_retries(&POST, Backend, V1, endpoint, &data, retries)
            .await
    }

    async fn upsert_file(&self, data: &File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(&PUT, Backend, V1, endpoint, &data).await
    }

    async fn upsert_file_with_retries(
        &self,
        data: &File,
        retries: usize,
    ) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request_with_retries(&PUT, Backend, V1, endpoint, &data, retries)
            .await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(&self, data: &FileId) -> Result<String, ApiError> {
        let endpoint = "/file";
        self.request(&DELETE, Backend, V1, endpoint, &data).await
    }

    async fn get_directory(
        &self,
        data: &Directory,
    ) -> Result<Vec<File>, ApiError> {
        let endpoint = "/directory";
        self.request(&GET, Backend, V1, endpoint, &data).await
    }

    async fn notify_runner(
        &self,
        data: UserPorts,
    ) -> Result<UserPorts, ApiError> {
        self.request(&POST, Runner, V1, "/ready", &data).await
    }
}

impl LexeApiClient {
    /// Tries to complete an API request, retrying up to `retries` times.
    async fn request_with_retries<D: Serialize, T: DeserializeOwned>(
        &self,
        method: &Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
        retries: usize,
    ) -> Result<T, ApiError> {
        let mut retry_timer = time::interval(RETRY_INTERVAL);

        // 'Do the retries first' and return early if successful.
        // If retries == 0 this block is a noop.
        for _ in 0..retries {
            let res = self.request(method, base, ver, endpoint, data).await;
            if res.is_ok() {
                return res;
            } else {
                // TODO log errors here

                // Since the first tick resolves immediately, and we tick only
                // on failures, the first failed attempt is immediately followed
                // up with second attempt (to encode that sometimes messages are
                // dropped during normal operation), but all following attempts
                // wait the full timeout (to encode that the node backend is
                // probably down so we want to wait a relatively long timeout).
                retry_timer.tick().await;
            }
        }

        // Do the 'main attempt'.
        self.request(method, base, ver, endpoint, &data).await
    }

    /// Executes an API request once.
    async fn request<D: Serialize, T: DeserializeOwned>(
        &self,
        method: &Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
    ) -> Result<T, ApiError> {
        // Node backend api is versioned but runner api is not
        let (base, ver) = match base {
            Backend => (&self.backend_url, ver.to_string()),
            Runner => (&self.runner_url, String::new()),
        };
        let mut url = format!("{}{}{}", base, ver, endpoint);

        // If GET, serialize the data in a query string
        let method = method.to_owned();
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
        let body = match method {
            PUT | POST => serde_json::to_string(data)?,
            _ => String::new(),
        };
        // println!("    Body: {}", body);

        let response =
            self.client.request(method, url).body(body).send().await?;

        if response.status().is_success() {
            // Uncomment for debugging
            // let text = response.text().await?;
            // println!("Response: {}", text);
            // serde_json::from_str(&text).map_err(|e| e.into())

            // Deserialize into JSON, return Ok(json)
            response.json().await.map_err(|e| e.into())
        } else {
            // Deserialize into String, return Err(ApiError::Server(string))
            Err(ApiError::Server(response.text().await?))
        }
    }
}
