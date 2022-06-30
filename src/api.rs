use std::env;

use http::Method;
use once_cell::sync::{Lazy, OnceCell};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use BaseUrl::*;

use crate::types::{Port, UserId};

/// The base url for the node-backend (persistence) API.
/// Can be overridden with BACKEND_URL env var.
static BACKEND_URL: Lazy<OnceCell<String>> = Lazy::new(|| {
    env::var("BACKEND_URL")
        .unwrap_or("http://127.0.0.1:3030/v1".to_string())
        .into()
});

/// The base url for the runner. Can be overridden with RUNNER_URL env var.
static RUNNER_URL: Lazy<OnceCell<String>> = Lazy::new(|| {
    env::var("RUNNER_URL")
        .unwrap_or("http://127.0.0.1:5050".to_string())
        .into()
});

/// Enumerates the base urls that can be used in an API call.
enum BaseUrl {
    Backend,
    Runner,
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

/// Query parameter struct for fetching with no data attached
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Query parameter struct for fetching by user id
#[derive(Serialize)]
pub struct GetByUserId {
    pub user_id: UserId,
}

/// Query parameter struct for fetching by user id and measurement
#[derive(Serialize)]
pub struct GetByUserIdAndMeasurement {
    pub user_id: UserId,
    pub measurement: String,
}

/// Query parameter struct for fetching by instance id
#[derive(Serialize)]
pub struct GetByInstanceId {
    pub instance_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub public_key: String,
    pub user_id: UserId,
}

pub async fn get_node(
    cli: &Client,
    user_id: UserId,
) -> Result<Option<Node>, ApiError> {
    request(cli, Method::GET, Backend, "/node", GetByUserId { user_id }).await
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub measurement: String,
    pub node_public_key: String,
}

pub async fn get_instance(
    cli: &Client,
    user_id: UserId,
    measurement: String,
) -> Result<Option<Instance>, ApiError> {
    let req = GetByUserIdAndMeasurement {
        user_id,
        measurement,
    };
    request(cli, Method::GET, Backend, "/instance", req).await
}

#[derive(Serialize, Deserialize)]
pub struct Enclave {
    pub id: String,
    pub seed: Vec<u8>,
    pub instance_id: String,
}

pub async fn get_enclave(
    cli: &Client,
    user_id: UserId,
    measurement: String,
) -> Result<Option<Enclave>, ApiError> {
    let req = GetByUserIdAndMeasurement {
        user_id,
        measurement,
    };
    request(cli, Method::GET, Backend, "/enclave", req).await
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceEnclave {
    pub node: Node,
    pub instance: Instance,
    pub enclave: Enclave,
}

pub async fn create_node_instance_enclave(
    cli: &Client,
    req: NodeInstanceEnclave,
) -> Result<NodeInstanceEnclave, ApiError> {
    let endpoint = "/acid/node_instance_enclave";
    request(cli, Method::POST, Backend, endpoint, req).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelMonitor {
    pub instance_id: String,
    pub tx_id: String,
    pub tx_index: i16,
    pub state: Vec<u8>,
}

pub async fn create_channel_monitor(
    cli: &Client,
    req: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(cli, Method::POST, Backend, "/channel_monitor", req).await
}

pub async fn get_channel_monitors(
    cli: &Client,
    instance_id: String,
) -> Result<Vec<ChannelMonitor>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(cli, Method::GET, Backend, "/channel_monitor", req).await
}

pub async fn update_channel_monitor(
    cli: &Client,
    req: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(cli, Method::PUT, Backend, "/channel_monitor", req).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelManager {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_channel_manager(
    cli: &Client,
    instance_id: String,
) -> Result<Option<ChannelManager>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(cli, Method::GET, Backend, "/channel_manager", req).await
}

pub async fn create_or_update_channel_manager(
    cli: &Client,
    req: ChannelManager,
) -> Result<ChannelManager, ApiError> {
    request(cli, Method::PUT, Backend, "/channel_manager", req).await
}

#[derive(Serialize, Deserialize)]
pub struct ProbabilisticScorer {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_probabilistic_scorer(
    cli: &Client,
    instance_id: String,
) -> Result<Option<ProbabilisticScorer>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(cli, Method::GET, Backend, "/probabilistic_scorer", req).await
}

pub async fn create_or_update_probabilistic_scorer(
    cli: &Client,
    ps: ProbabilisticScorer,
) -> Result<ProbabilisticScorer, ApiError> {
    request(cli, Method::PUT, Backend, "/probabilistic_scorer", ps).await
}

#[derive(Serialize, Deserialize)]
pub struct NetworkGraph {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_network_graph(
    cli: &Client,
    instance_id: String,
) -> Result<Option<NetworkGraph>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(cli, Method::GET, Backend, "/network_graph", req).await
}

pub async fn create_or_update_network_graph(
    cli: &Client,
    ng: NetworkGraph,
) -> Result<NetworkGraph, ApiError> {
    request(cli, Method::PUT, Backend, "/network_graph", ng).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelPeer {
    pub instance_id: String,
    pub peer_public_key: String,
    pub peer_address: String,
}

pub async fn create_channel_peer(
    cli: &Client,
    channel_peer: ChannelPeer,
) -> Result<ChannelPeer, ApiError> {
    request(cli, Method::POST, Backend, "/channel_peer", channel_peer).await
}

pub async fn get_channel_peers(
    cli: &Client,
    instance_id: String,
) -> Result<Vec<ChannelPeer>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(cli, Method::GET, Backend, "/channel_peer", req).await
}

#[derive(Serialize, Deserialize)]
pub struct UserPort {
    pub user_id: UserId,
    pub port: Port,
}

pub async fn notify_runner(
    cli: &Client,
    req: UserPort,
) -> Result<UserPort, ApiError> {
    request(cli, Method::POST, Runner, "/ready", req).await
}

/// Builds and executes the API request
async fn request<D: Serialize, T: DeserializeOwned>(
    cli: &Client,
    method: Method,
    base_url: BaseUrl,
    endpoint: &str,
    data: D,
) -> Result<T, ApiError> {
    let base = match base_url {
        BaseUrl::Backend => BACKEND_URL.get().unwrap(),
        BaseUrl::Runner => RUNNER_URL.get().unwrap(),
    };
    let mut url = format!("{}{}", base, endpoint);

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
    println!("{} {}", method, url);

    // If PUT or POST, serialize the data in the request body
    let body = match method {
        Method::PUT | Method::POST => serde_json::to_string(&data)?,
        _ => String::new(),
    };
    // println!("    Body: {}", body);

    let response = cli.request(method, url).body(body).send().await?;

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
