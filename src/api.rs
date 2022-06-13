use http::Method;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const API_URL: &str = "http://127.0.0.1:3030/v1";

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
    pub user_id: i64,
}

/// Query parameter struct for fetching by user id and measurement
#[derive(Serialize)]
pub struct GetByUserIdAndMeasurement {
    pub user_id: i64,
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
    pub user_id: i64,
}

pub async fn get_node(
    client: &Client,
    user_id: i64,
) -> Result<Option<Node>, ApiError> {
    request(client, Method::GET, "/node", GetByUserId { user_id }).await
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub measurement: String,
    pub node_public_key: String,
    pub seed: Vec<u8>,
}

pub async fn get_instance(
    client: &Client,
    user_id: i64,
    measurement: String,
) -> Result<Option<Instance>, ApiError> {
    let req = GetByUserIdAndMeasurement {
        user_id,
        measurement,
    };
    request(client, Method::GET, "/instance", req).await
}

#[derive(Serialize, Deserialize)]
pub struct NodeAndInstance {
    pub node: Node,
    pub instance: Instance,
}

pub async fn create_node_and_instance(
    client: &Client,
    req: NodeAndInstance,
) -> Result<Node, ApiError> {
    request(client, Method::POST, "/acid/node_and_instance", req).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelMonitor {
    pub instance_id: String,
    pub tx_id: String,
    pub tx_index: i16,
    pub state: Vec<u8>,
}

pub async fn create_channel_monitor(
    client: &Client,
    channel_monitor: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(client, Method::POST, "/channel_monitor", channel_monitor).await
}

pub async fn get_channel_monitors(
    client: &Client,
    instance_id: String,
) -> Result<Vec<ChannelMonitor>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(client, Method::GET, "/channel_monitor", req).await
}

pub async fn update_channel_monitor(
    client: &Client,
    channel_monitor: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(client, Method::PUT, "/channel_monitor", channel_monitor).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelManager {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_channel_manager(
    client: &Client,
    instance_id: String,
) -> Result<Option<ChannelManager>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(client, Method::GET, "/channel_manager", req).await
}

pub async fn create_or_update_channel_manager(
    client: &Client,
    channel_manager: ChannelManager,
) -> Result<ChannelManager, ApiError> {
    request(client, Method::PUT, "/channel_manager", channel_manager).await
}

#[derive(Serialize, Deserialize)]
pub struct ProbabilisticScorer {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_probabilistic_scorer(
    client: &Client,
    instance_id: String,
) -> Result<Option<ProbabilisticScorer>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(client, Method::GET, "/probabilistic_scorer", req).await
}

pub async fn create_or_update_probabilistic_scorer(
    client: &Client,
    ps: ProbabilisticScorer,
) -> Result<ProbabilisticScorer, ApiError> {
    request(client, Method::PUT, "/probabilistic_scorer", ps).await
}

#[derive(Serialize, Deserialize)]
pub struct NetworkGraph {
    pub instance_id: String,
    pub state: Vec<u8>,
}

pub async fn get_network_graph(
    client: &Client,
    instance_id: String,
) -> Result<Option<NetworkGraph>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(client, Method::GET, "/network_graph", req).await
}

pub async fn create_or_update_network_graph(
    client: &Client,
    ng: NetworkGraph,
) -> Result<NetworkGraph, ApiError> {
    request(client, Method::PUT, "/network_graph", ng).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelPeer {
    pub instance_id: String,
    pub peer_public_key: String,
    pub peer_address: String,
}

pub async fn create_channel_peer(
    client: &Client,
    channel_peer: ChannelPeer,
) -> Result<ChannelPeer, ApiError> {
    request(client, Method::POST, "/channel_peer", channel_peer).await
}

pub async fn get_channel_peers(
    client: &Client,
    instance_id: String,
) -> Result<Vec<ChannelPeer>, ApiError> {
    let req = GetByInstanceId { instance_id };
    request(client, Method::GET, "/channel_peer", req).await
}

/// Builds and executes the API request
async fn request<D: Serialize, T: DeserializeOwned>(
    client: &Client,
    method: Method,
    endpoint: &str,
    data: D,
) -> Result<T, ApiError> {
    let mut url = format!("{}{}", API_URL, endpoint);

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

    let response = client.request(method, url).body(body).send().await?;

    if response.status().is_success() {
        // Deserialize into JSON, return Ok(json)
        response.json().await.map_err(|e| e.into())
    } else {
        // Deserialize into String, return Err(ApiError::Server(string))
        Err(ApiError::Server(response.text().await?))
    }
}
