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
use tracing::debug;

use crate::api::*;

const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ATTEMPTS: usize = 3;

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

use ApiVersion::*;
use BaseUrl::*;

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
        self.request(&Method::GET, Backend, V1, "/node", &data)
            .await
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
            .request(&Method::GET, Backend, V1, "/instance", &data)
            .await?;

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
        self.request(&Method::GET, Backend, V1, "/sealed_seed", &data)
            .await
    }

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, ApiError> {
        let endpoint = "/acid/node_instance_seed";
        self.request(&Method::POST, Backend, V1, endpoint, &data)
            .await
    }

    async fn get_file(&self, data: FileId) -> Result<Option<File>, ApiError> {
        let endpoint = "/file";
        self.request(&Method::GET, Backend, V1, endpoint, &data)
            .await
    }

    async fn create_file(&self, data: File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(&Method::POST, Backend, V1, endpoint, &data)
            .await
    }

    async fn upsert_file(&self, data: File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(&Method::PUT, Backend, V1, endpoint, &data)
            .await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    async fn delete_file(&self, data: FileId) -> Result<String, ApiError> {
        let endpoint = "/file";
        self.request(&Method::DELETE, Backend, V1, endpoint, &data)
            .await
    }

    async fn get_directory(
        &self,
        data: Directory,
    ) -> Result<Vec<File>, ApiError> {
        let endpoint = "/directory";
        self.request(&Method::GET, Backend, V1, endpoint, &data)
            .await
    }

    async fn notify_runner(
        &self,
        data: UserPorts,
    ) -> Result<UserPorts, ApiError> {
        self.request(&Method::POST, Runner, V1, "/ready", &data)
            .await
    }
}

impl LexeApiClient {
    /// Tries to complete an API request, making up to `MAX_ATTEMPTS` attempts.
    async fn request<D: Serialize, T: DeserializeOwned>(
        &self,
        method: &Method,
        base: BaseUrl,
        ver: ApiVersion,
        endpoint: &str,
        data: &D,
    ) -> Result<T, ApiError> {
        // Try the first n-1 times, return early if successful
        for _ in 0..MAX_ATTEMPTS - 1 {
            let res = self.execute(method, base, ver, endpoint, data).await;
            if res.is_ok() {
                return res;
            }
        }

        // Last try
        self.execute(method, base, ver, endpoint, &data).await
    }

    /// Executes an API request once.
    async fn execute<D: Serialize, T: DeserializeOwned>(
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
            Method::GET => Some(serde_qs::to_string(data)?),
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
            Method::PUT | Method::POST => serde_json::to_string(data)?,
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
