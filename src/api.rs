use http::Method;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const API_URL: &str = "http://127.0.0.1:3030/v1";

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Reqwest error")]
    ReqwestError(#[from] reqwest::Error),
    #[error("JSON serialization error")]
    JsonSerializeError(#[from] serde_json::Error),
}

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
    request(client, Method::GET, "/node", EmptyBody).await
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

pub async fn update_channel_monitor(
    client: &Client,
    channel_monitor: ChannelMonitor,
) -> Result<ChannelMonitor, ApiError> {
    request(client, Method::PUT, "/channel_monitor", channel_monitor).await
}

/// An empty request body which can be used for e.g. GET requests
#[derive(Serialize)]
struct EmptyBody;

/// Builds and executes the API request
async fn request<B: Serialize, T: DeserializeOwned>(
    client: &Client,
    method: Method,
    endpoint: &str,
    body: B,
) -> Result<T, ApiError> {
    let url = format!("{}{}", API_URL, endpoint);
    let body = serde_json::to_string(&body)?;

    client
        .request(method, url)
        .body(body)
        .send()
        .await?
        .json()
        .await
        .map_err(|e| e.into())
}
