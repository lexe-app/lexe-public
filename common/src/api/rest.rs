use std::cmp::min;
use std::time::Duration;

use bytes::Bytes;
use http::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time;
use tracing::{debug, trace, warn};

// Default parameters
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

// Exponential backup
const INITIAL_WAIT_MS: u64 = 250;
const MAXIMUM_WAIT_MS: u64 = 32_000;
const EXP_BASE: u64 = 2;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum RestError {
    #[error("Reqwest error: {0}")]
    Reqwest(String),

    #[error("JSON serialization error: {0}")]
    JsonSerialization(String),

    #[error("Query string serialization error: {0}")]
    QueryStringSerialization(String),
}

// Have to serialize to string because these error types don't implement ser/de
impl From<reqwest::Error> for RestError {
    fn from(err: reqwest::Error) -> Self {
        Self::Reqwest(format!("{err:#}"))
    }
}
impl From<serde_json::Error> for RestError {
    fn from(err: serde_json::Error) -> Self {
        Self::Reqwest(format!("{err:#}"))
    }
}
impl From<serde_qs::Error> for RestError {
    fn from(err: serde_qs::Error) -> Self {
        Self::Reqwest(format!("{err:#}"))
    }
}

struct RequestParts {
    method: Method,
    url: String,
    body: Bytes,
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

    /// Makes an API request with 0 retries.
    pub async fn request<D, T, E>(
        &self,
        method: Method,
        url: String,
        data: &D,
    ) -> Result<T, E>
    where
        D: Serialize,
        T: DeserializeOwned,
        E: DeserializeOwned + From<RestError>,
    {
        self.request_with_retries(method, url, data, 0).await
    }

    /// Makes an API request, retrying up to `retries` times.
    pub async fn request_with_retries<D, T, E>(
        &self,
        method: Method,
        url: String,
        data: &D,
        retries: usize,
    ) -> Result<T, E>
    where
        D: Serialize,
        T: DeserializeOwned,
        E: DeserializeOwned + From<RestError>,
    {
        // Serialize request parts
        let parts = self.serialize_parts(method, url, data)?;

        // Exponential backup
        let mut backup_durations = (0..)
            .map(|index| INITIAL_WAIT_MS * EXP_BASE.pow(index))
            .map(|wait| min(wait, MAXIMUM_WAIT_MS))
            .map(Duration::from_millis);

        // Do the 'retries' first and return early if successful.
        // This block is a noop if retries == 0.
        for _ in 0..retries {
            let res = self.send_and_deserialize(&parts).await;
            if res.is_ok() {
                return res;
            } else {
                let method = &parts.method;
                let url = &parts.url;
                warn!("{method} {url} failed.");

                time::sleep(backup_durations.next().unwrap()).await;
            }
        }

        // Do the 'main' attempt.
        self.send_and_deserialize(&parts).await
    }

    /// Constructs the final, serialized parts of a [`reqwest::Request`] given
    /// an HTTP method and url. The given url should include the base, version,
    /// and endpoint, but should NOT include the query string.
    ///
    /// Example: "http://127.0.0.1:3030/v1/file"
    fn serialize_parts<D: Serialize>(
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

    /// Build a [`reqwest::Request`] from [`RequestParts`], send it, and
    /// deserialize into the expected struct or an error message depending on
    /// the response status.
    async fn send_and_deserialize<T, E>(
        &self,
        parts: &RequestParts,
    ) -> Result<T, E>
    where
        T: DeserializeOwned,
        E: DeserializeOwned + From<RestError>,
    {
        let response = self
            .client
            // Method doesn't implement Copy
            .request(parts.method.clone(), &parts.url)
            // body is Bytes which can be cheaply cloned
            .body(parts.body.clone())
            .send()
            .await
            .map_err(RestError::from)?;

        if response.status().is_success() {
            // Uncomment for debugging
            // let text = response.text().await?;
            // println!("Response: {}", text);
            // serde_json::from_str(&text).map_err(|e| e.into())

            // Deserialize into Ok variant, return Ok(json)
            response
                .json::<T>()
                .await
                .map_err(RestError::from)
                .map_err(E::from)
        } else {
            // Deserialize into Err variant, return Err(json)
            Err(response.json::<E>().await.map_err(RestError::from)?)
        }
    }
}
