use http::Method;
use reqwest::Client;
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
) -> Result<String, ApiError> {
    let method = Method::POST;
    let endpoint = "/node";
    let url = format!("{}{}", API_URL, endpoint);
    let body = serde_json::to_string(&node)?;

    // Debugging
    // let debug_resp: String = client
    //     .request(method.clone(), url.clone())
    //     .send()
    //     .await?
    //     .text_with_charset("utf-8")
    //     .await?;
    // panic!("{:#?}", debug_resp);

    client
        .request(method, url)
        .body(body)
        .send()
        .await?
        .text()
        .await
        .map_err(|e| e.into())
}

pub async fn get_node(client: &Client) -> Result<Option<Node>, ApiError> {
    let method = Method::GET;
    let endpoint = "/node";
    let url = format!("{}{}", API_URL, endpoint);

    client
        .request(method, url)
        .send()
        .await?
        .json()
        .await
        .map_err(|e| e.into())
}
