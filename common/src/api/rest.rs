use std::time::Duration;

use bytes::Bytes;
use http::Method;
use thiserror::Error;
use serde::de::DeserializeOwned;

// Default parameters
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

#[derive(Error, Debug)]
pub enum RestError {
    #[error("Reqwest error")]
    Reqwest(#[from] reqwest::Error),

    #[error("JSON serialization error")]
    JsonSerialization(#[from] serde_json::Error),

    #[error("Query string serialization error")]
    QueryStringSerialization(#[from] serde_qs::Error),

    #[error("Server Error: {0}")]
    Server(String),
}

pub struct RequestParts {
    pub method: Method,
    pub url: String,
    pub body: Bytes,
}

pub struct RestClient {
    client: reqwest::Client,
}

impl Default for RestClient {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_REQUEST_TIMEOUT)
            .build()
            .expect("Failed to build reqwest Client");
        Self { client }
    }
}

impl RestClient {
    pub fn new() -> Self {
        Self::default()
    }

    // TODO(max): This might not need to be pub?
    /// Build a [`reqwest::Request`] from [`RequestParts`], send it, and
    /// deserialize into the expected struct or an error message depending on
    /// the response status.
    pub async fn send_request<T: DeserializeOwned>(
        &self,
        parts: &RequestParts,
    ) -> Result<T, RestError> {
        let response = self
            .client
            // Method doesn't implement Copy
            .request(parts.method.clone(), &parts.url)
            // body is Bytes which can be cheaply cloned
            .body(parts.body.clone())
            .send()
            .await?;

        if response.status().is_success() {
            // Uncomment for debugging
            // let text = response.text().await?;
            // println!("Response: {}", text);
            // serde_json::from_str(&text).map_err(|e| e.into())

            // Deserialize into JSON, return Ok(json)
            response.json().await.map_err(|e| e.into())
        } else {
            // Deserialize into String, return Err(RestError::Server(string))
            Err(RestError::Server(response.text().await?))
        }
    }
}
