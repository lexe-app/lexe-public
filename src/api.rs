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

/// Struct which can be used to make requests with no data attached
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Struct which can be used to query data based on a node's public key
#[derive(Serialize)]
pub struct GetByPublicKey {
    pub public_key: String,
}
// TODO impl From<PublicKey> for GetByPublicKey

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub public_key: String,
    pub keys_seed: Vec<u8>,
}

pub async fn create_node(
    client: &Client,
    node: Node,
) -> Result<Node, ApiError> {
    request(client, Method::POST, "/node", node).await
}

pub async fn get_node(client: &Client) -> Result<Option<Node>, ApiError> {
    request(client, Method::GET, "/node", EmptyData {}).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelMonitor {
    pub node_public_key: String,
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
    public_key: String,
) -> Result<Vec<ChannelMonitor>, ApiError> {
    let get_by_pubkey = GetByPublicKey { public_key };
    request(client, Method::GET, "/channel_monitor", get_by_pubkey).await
}

pub async fn update_channel_monitor(
    client: &Client,
    channel_monitor: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(client, Method::PUT, "/channel_monitor", channel_monitor).await
}

#[derive(Serialize, Deserialize)]
pub struct ChannelManager {
    pub node_public_key: String,
    pub state: Vec<u8>,
}

pub async fn get_channel_manager(
    client: &Client,
    public_key: String,
) -> Result<Option<ChannelManager>, ApiError> {
    let get_by_pubkey = GetByPublicKey { public_key };
    request(client, Method::GET, "/channel_manager", get_by_pubkey).await
}

pub async fn update_channel_manager(
    client: &Client,
    channel_manager: ChannelManager,
) -> Result<ChannelManager, ApiError> {
    request(client, Method::POST, "/channel_manager", channel_manager).await
}

#[derive(Serialize, Deserialize)]
pub struct ProbabilisticScorer {
    pub node_public_key: String,
    pub state: Vec<u8>,
}

pub async fn get_probabilistic_scorer(
    client: &Client,
    public_key: String,
) -> Result<Option<ProbabilisticScorer>, ApiError> {
    let get_by_pubkey = GetByPublicKey { public_key };
    request(client, Method::GET, "/probabilistic_scorer", get_by_pubkey).await
}

pub async fn update_probabilistic_scorer(
    client: &Client,
    ps: ProbabilisticScorer,
) -> Result<ProbabilisticScorer, ApiError> {
    request(client, Method::PUT, "/probabilistic_scorer", ps).await
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
