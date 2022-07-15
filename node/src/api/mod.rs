use std::env;
use std::fmt::{self, Display};

use http::Method;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use tracing::debug;
use ApiVersion::*;
use BaseUrl::*;

use crate::types::UserId;

mod models;

pub use models::*;

/// The base url for the node-backend (persistence) API.
/// Can be overridden with BACKEND_URL env var.
static BACKEND_URL: Lazy<String> = Lazy::new(|| {
    env::var("BACKEND_URL")
        .unwrap_or_else(|_e| "http://127.0.0.1:3030".to_string())
});

/// The base url for the runner. Can be overridden with RUNNER_URL env var.
static RUNNER_URL: Lazy<String> = Lazy::new(|| {
    env::var("RUNNER_URL")
        .unwrap_or_else(|_e| "http://127.0.0.1:5050".to_string())
});

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
pub struct ApiClient {
    client: Client,
}

impl From<Client> for ApiClient {
    fn from(client: Client) -> Self {
        Self { client }
    }
}

impl ApiClient {
    pub async fn get_node(
        &self,
        user_id: UserId,
    ) -> Result<Option<Node>, ApiError> {
        let req = GetByUserId { user_id };
        self.request(Method::GET, Backend, V1, "/node", req).await
    }

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

    pub async fn create_channel_monitor(
        &self,
        req: ChannelMonitor,
    ) -> Result<ChannelMonitor, ApiError> {
        self.request(Method::POST, Backend, V1, "/channel_monitor", req)
            .await
    }

    pub async fn get_channel_monitors(
        &self,
        instance_id: String,
    ) -> Result<Vec<ChannelMonitor>, ApiError> {
        let req = GetByInstanceId { instance_id };
        self.request(Method::GET, Backend, V1, "/channel_monitor", req)
            .await
    }

    pub async fn update_channel_monitor(
        &self,
        req: ChannelMonitor,
    ) -> Result<ChannelMonitor, ApiError> {
        self.request(Method::PUT, Backend, V1, "/channel_monitor", req)
            .await
    }

    pub async fn get_channel_manager(
        &self,
        instance_id: String,
    ) -> Result<Option<ChannelManager>, ApiError> {
        let req = GetByInstanceId { instance_id };
        self.request(Method::GET, Backend, V1, "/channel_manager", req)
            .await
    }

    pub async fn create_or_update_channel_manager(
        &self,
        req: ChannelManager,
    ) -> Result<ChannelManager, ApiError> {
        self.request(Method::PUT, Backend, V1, "/channel_manager", req)
            .await
    }

    pub async fn get_probabilistic_scorer(
        &self,
        instance_id: String,
    ) -> Result<Option<ProbabilisticScorer>, ApiError> {
        let req = GetByInstanceId { instance_id };
        self.request(Method::GET, Backend, V1, "/probabilistic_scorer", req)
            .await
    }

    pub async fn create_or_update_probabilistic_scorer(
        &self,
        ps: ProbabilisticScorer,
    ) -> Result<ProbabilisticScorer, ApiError> {
        self.request(Method::PUT, Backend, V1, "/probabilistic_scorer", ps)
            .await
    }

    pub async fn get_network_graph(
        &self,
        instance_id: String,
    ) -> Result<Option<NetworkGraph>, ApiError> {
        let req = GetByInstanceId { instance_id };
        self.request(Method::GET, Backend, V1, "/network_graph", req)
            .await
    }

    pub async fn create_or_update_network_graph(
        &self,
        ng: NetworkGraph,
    ) -> Result<NetworkGraph, ApiError> {
        self.request(Method::PUT, Backend, V1, "/network_graph", ng)
            .await
    }

    #[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
    pub async fn create_channel_peer(
        &self,
        req: ChannelPeer,
    ) -> Result<ChannelPeer, ApiError> {
        self.request(Method::POST, Backend, V1, "/channel_peer", req)
            .await
    }

    pub async fn get_channel_peers(
        &self,
        instance_id: String,
    ) -> Result<Vec<ChannelPeer>, ApiError> {
        let req = GetByInstanceId { instance_id };
        self.request(Method::GET, Backend, V1, "/channel_peer", req)
            .await
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
            Backend => (&*BACKEND_URL, api_version.to_string()),
            Runner => (&*RUNNER_URL, String::new()),
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
