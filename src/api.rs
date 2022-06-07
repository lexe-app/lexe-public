use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

const API_URL: &str = "localhost:3030/v1";

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Reqwest error")]
    ReqwestError(#[from] reqwest::Error),
}

#[derive(Deserialize)]
pub struct Node {
    pub public_key: Vec<u8>,
    pub keys_seed: Vec<u8>,
}

pub async fn _get_node(client: &Client) -> Result<Node, ApiError> {
    let endpoint = "/node";
    let url = format!("{}{}", API_URL, endpoint);

    client
        .get(url)
        .send()
        .await?
        .json()
        .await
        .map_err(|e| e.into())
}
