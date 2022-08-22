use std::time::Duration;

use bytes::Bytes;
use http::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tracing::{debug, trace};

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

// TODO(max): Does this need to be public?
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

    // TODO(max): Does this need to be public?
    /// Constructs the final, serialized parts of a [`reqwest::Request`] given
    /// an HTTP method and url. The url should include the base, version, and
    /// endpoint, but should NOT include the query string.
    /// Example: "http://127.0.0.1:3030/v1/file"
    pub fn serialize_parts<D: Serialize>(
        &self,
        method: Method,
        mut url: String,
        data: &D,
    ) -> Result<RequestParts, RestError> {
        // If GET, serialize the data in a query string
        let query_str = match method {
            GET => Some(serde_qs::to_string(data)?),
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
        let body_str = match method {
            PUT | POST => serde_json::to_string(data)?,
            _ => String::new(),
        };
        trace!(%body_str);
        let body = Bytes::from(body_str);

        Ok(RequestParts { method, url, body })
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
