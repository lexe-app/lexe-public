use std::fmt::{self, Display};
use std::time::Duration;

use http::Method;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use tracing::debug;
use ApiVersion::*;
use BaseUrl::*;

#[cfg(not(test))]
use crate::types::UserId;

mod models;

pub use models::*;

const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Enumerates the base urls that can be used in an API call.
enum BaseUrl {
    Backend,
    Runner,
}

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

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Reqwest error")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON serialization error")]
    JsonSerialization(#[from] serde_json::Error),
    #[error("Query string serialization error")]
    QueryStringSerialization(#[from] serde_qs::Error),
    #[error("Server Error: {0}")]
    Server(String),
}

#[derive(Clone)]
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

    #[cfg(not(test))]
    pub async fn get_node(
        &self,
        user_id: UserId,
    ) -> Result<Option<Node>, ApiError> {
        let req = GetByUserId { user_id };
        self.request(Method::GET, Backend, V1, "/node", req).await
    }

    #[cfg(not(test))]
    pub async fn get_instance(
        &self,
        user_id: UserId,
        measurement: String,
    ) -> Result<Option<Instance>, ApiError> {
        let req = GetByUserIdAndMeasurement {
            user_id,
            measurement,
        };
        self.request(Method::GET, Backend, V1, "/instance", req)
            .await
    }

    #[cfg(not(test))]
    pub async fn get_enclave(
        &self,
        user_id: UserId,
        measurement: String,
    ) -> Result<Option<Enclave>, ApiError> {
        let req = GetByUserIdAndMeasurement {
            user_id,
            measurement,
        };
        self.request(Method::GET, Backend, V1, "/enclave", req)
            .await
    }

    pub async fn create_node_instance_enclave(
        &self,
        req: NodeInstanceEnclave,
    ) -> Result<NodeInstanceEnclave, ApiError> {
        let endpoint = "/acid/node_instance_enclave";
        self.request(Method::POST, Backend, V1, endpoint, req).await
    }

    pub async fn get_file(
        &self,
        req: FileId,
    ) -> Result<Option<File>, ApiError> {
        let endpoint = "/file";
        self.request(Method::GET, Backend, V1, endpoint, req).await
    }

    pub async fn create_file(&self, req: File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(Method::POST, Backend, V1, endpoint, req).await
    }

    pub async fn upsert_file(&self, req: File) -> Result<File, ApiError> {
        let endpoint = "/file";
        self.request(Method::PUT, Backend, V1, endpoint, req).await
    }

    // TODO We want to delete channel peers / monitors when channels close
    /// Returns "OK" if exactly one row was deleted.
    #[allow(dead_code)]
    pub async fn delete_file(&self, req: FileId) -> Result<String, ApiError> {
        let endpoint = "/file";
        self.request(Method::DELETE, Backend, V1, endpoint, req)
            .await
    }

    pub async fn get_directory(
        &self,
        req: DirectoryId,
    ) -> Result<Vec<File>, ApiError> {
        let endpoint = "/directory";
        self.request(Method::GET, Backend, V1, endpoint, req).await
    }

    pub async fn notify_runner(
        &self,
        req: UserPort,
    ) -> Result<UserPort, ApiError> {
        self.request(Method::POST, Runner, V1, "/ready", req).await
    }

    /// Builds and executes the API request
    async fn request<D: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        base_url: BaseUrl,
        api_version: ApiVersion,
        endpoint: &str,
        data: D,
    ) -> Result<T, ApiError> {
        // Node backend api is versioned but runner api is not
        let (base, version) = match base_url {
            Backend => (&self.backend_url, api_version.to_string()),
            Runner => (&self.runner_url, String::new()),
        };
        let mut url = format!("{}{}{}", base, version, endpoint);

        // If GET, serialize the data in a query string
        let query_str = match method {
            Method::GET => Some(serde_qs::to_string(&data)?),
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
            Method::PUT | Method::POST => serde_json::to_string(&data)?,
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
