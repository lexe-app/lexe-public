use http::Method;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const API_URL: &str = "localhost:3030/v1";

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

pub async fn create_node(client: &Client, node: Node) -> Result<(), ApiError> {
    let method = Method::POST;
    let endpoint = "/node";
    let url = format!("{}{}", API_URL, endpoint);
    let body = serde_json::to_string(&node)?;

    client
        .request(method, url)
        .body(body)
        .send()
        .await?
        .json()
        .await
        .map_err(|e| e.into())
}

pub async fn get_node(client: &Client) -> Result<Option<Node>, ApiError> {
    let method = Method::POST;
    let endpoint = "/node";
    let url = format!("{}{}", API_URL, endpoint);

    // TODO Detect NotFound and return Option<Node> instead, Ok/Err should
    // indicate whether the API call succeeded or not
    client
        .request(method, url)
        .send()
        .await?
        .json()
        .await
        .map_err(|e| e.into())
}
