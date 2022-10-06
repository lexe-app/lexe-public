use std::time::Duration;

use bytes::Bytes;
use http::header::{HeaderValue, CONTENT_TYPE};
use http::response::Response;
use http::status::StatusCode;
use http::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::time;
use tracing::{debug, error, trace, warn};
use warp::hyper::Body;
use warp::Rejection;

use crate::api::error::{
    CommonError, ErrorCode, ErrorResponse, HasStatusCode, ServiceApiError,
};
use crate::backoff;

// Default parameters
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

/// A warp helper that converts `Result<T, E>` into [`Response<Body>`].
/// This function should be used after all *fallible* warp handlers because:
///
/// 1) `RestClient::send_and_deserialize` relies on the HTTP status code to
///    determine whether a response should be deserialized as the requested
///    object or as the error type. This function handles this automatically and
///    consistently across all Lexe APIs.
/// 2) It saves time; there is no need to call reply::json(&resp) in every warp
///    handler or to manually set the error code in every response.
/// 3) Removing the [`warp::Reply`] serialization step from the warp handlers
///    allows each handler to be independently unit and integration tested.
///
/// For infallible handlers, use [`into_succ_response`] instead.
///
/// ## Usage
///
/// ```ignore
/// let status = warp::path("status")
///     .and(warp::get())
///     .and(warp::query())
///     .and(inject::user_pk(user_pk))
///     .then(host::status)
///     .map(into_response);
/// ```
pub fn into_response<T: Serialize, E: HasStatusCode + Into<ErrorResponse>>(
    reply_res: Result<T, E>,
) -> Response<Body> {
    match reply_res {
        Ok(data) => build_json_response(StatusCode::OK, &data),
        Err(err) => build_json_response(err.get_status_code(), &err.into()),
    }
}

/// Like [`into_response`], but converts `T` into [`Response<Body>`]. This fn
/// should be used for the same reasons that [`into_response`] is used, but
/// applies only to *infallible* handlers.
///
/// ## Usage
///
/// ```ignore
/// let node_info = warp::path("node_info")
///     .and(warp::get())
///     .and(inject::channel_manager(channel_manager.clone()))
///     .and(inject::peer_manager(peer_manager))
///     .map(command::node_info)
///     .map(into_succ_response);
/// ```
pub fn into_succ_response<T: Serialize>(data: T) -> Response<Body> {
    build_json_response(StatusCode::OK, &data)
}

/// A warp helper for recovering one of our [`api::error`](crate::api::error)
/// types if it was emitted from an intermediate filter's rejection and then
/// converting into the standard json error response.
///
/// ## Usage
///
/// ```ignore
/// let root = warp::path::end()
///     .then(handlers::root);
///
/// let foo = warp::path("foo")
///     .and(warp::get())
///     // Some custom filter returns a `warp::reject::custom` around one of our
///     // error types.
///     .and(|| warp::reject::custom(GatewayApiError { .. }))
///     .then(handlers::foo)
///     .map(into_response);
///
/// root.or(foo)
///     // recover the `GatewayApiError` from above and return standard json
///     // error response
///     .recover(recover_error_response::<GatewayApiError>)
/// ```
pub async fn recover_error_response<
    E: Clone
        + HasStatusCode
        + Into<ErrorResponse>
        + warp::reject::Reject
        + 'static,
>(
    err: Rejection,
) -> Result<Response<Body>, Rejection> {
    if let Some(err) = err.find::<E>() {
        let status = err.get_status_code();
        // TODO(phlip9): find returns &E... figure out how to remove clone
        let err: ErrorResponse = err.clone().into();
        Ok(build_json_response(status, &err))
    } else {
        Err(err)
    }
}

/// Constructs a JSON [`Response<Body>`] from the given data and status code.
/// If serialization fails for some reason (unlikely), log the error,
/// default to an empty body, and override the status code to 500.
fn build_json_response<T: Serialize>(
    mut status: StatusCode,
    data: &T,
) -> Response<Body> {
    let body = serde_json::to_vec(data)
        .map(Body::from)
        .unwrap_or_else(|e| {
            error!("Couldn't serialize response: {e:#}");
            status = StatusCode::INTERNAL_SERVER_ERROR;
            Body::empty()
        });

    Response::builder()
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .status(status)
        .body(body)
        // Only the header could have errored by this point
        .expect("Invalid hard-coded header")
}

struct RequestParts {
    method: Method,
    url_with_qs: String,
    body: Bytes,
}

#[derive(Clone)]
/// A generic RestClient. [`reqwest::Client`] holds an [`Arc`] internally, so
/// likewise, [`RestClient`] can be cloned and used directly, without [`Arc`].
///
/// [`Arc`]: std::sync::Arc
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

    pub fn from_preconfigured_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Makes an API request with 0 retries.
    ///
    /// The given url should include the base, version, and endpoint, but should
    /// NOT include the query string. Example: "http://127.0.0.1:3030/v1/file"
    ///
    /// Pass in [`EmptyData`] if you need an empty querystring / body.
    ///
    /// [`EmptyData`]: crate::api::qs::EmptyData
    pub async fn request<D, T, E>(
        &self,
        method: Method,
        url: String,
        data: &D,
    ) -> Result<T, E>
    where
        D: Serialize,
        T: DeserializeOwned,
        E: ServiceApiError,
    {
        self.request_with_retries(method, url, data, 0, None).await
    }

    /// Makes an API request, retrying up to `retries` times.
    ///
    /// The given url should include the base, version, and endpoint, but should
    /// NOT include the query string. Example: "http://127.0.0.1:3030/v1/file"
    ///
    /// Using `stop_codes` the client can be can be configured to quit early if
    /// the service returned any of the given error codes.
    pub async fn request_with_retries<D, T, E>(
        &self,
        method: Method,
        url: String,
        data: &D,
        retries: usize,
        stop_codes: Option<Vec<ErrorCode>>,
    ) -> Result<T, E>
    where
        D: Serialize,
        T: DeserializeOwned,
        E: ServiceApiError,
    {
        // Serialize request parts
        let parts = self.serialize_parts(method, url, data)?;

        // Exponential backoff
        let mut backoff_durations = backoff::get_backoff_iter();

        // Do the 'retries' first and return early if successful,
        // or if we received an error with one of the specified codes.
        // This block is a noop if retries == 0.
        for _ in 0..retries {
            match self.send_and_deserialize::<T, E>(&parts).await {
                Ok(data) => return Ok(data),
                Err(e) => {
                    let method = &parts.method;
                    let url_with_qs = &parts.url_with_qs;
                    warn!("{method} {url_with_qs} failed.");

                    if let Some(ref stop_codes) = stop_codes {
                        if stop_codes.contains(&e.to_code()) {
                            return Err(e);
                        }
                    }

                    time::sleep(backoff_durations.next().unwrap()).await;
                }
            }
        }

        // Do the 'main' attempt.
        self.send_and_deserialize(&parts).await
    }

    /// Constructs the serialized, reusable parts of a [`reqwest::Request`]
    /// given an HTTP method and url.
    ///
    /// The given url should include the base, version, and endpoint, but should
    /// NOT include the query string. Example: "http://127.0.0.1:3030/v1/file"
    fn serialize_parts<D: Serialize>(
        &self,
        method: Method,
        url: String,
        data: &D,
    ) -> Result<RequestParts, CommonError> {
        // If GET, serialize the data in a query string
        let query_str = match method {
            GET => Some(serde_qs::to_string(data)?),
            _ => None,
        };
        // Construct manually since RequestBuilder.param() API is unwieldy
        let url_with_qs = match query_str {
            Some(qs) if !qs.is_empty() => format!("{url}?{qs}"),
            _ => url,
        };
        debug!(%method, %url_with_qs, "sending request");

        // If PUT or POST, serialize the data in the request body
        let body_str = match method {
            PUT | POST => serde_json::to_string(data)?,
            _ => String::new(),
        };
        trace!(%body_str);
        let body = Bytes::from(body_str);

        Ok(RequestParts {
            method,
            url_with_qs,
            body,
        })
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
        E: ServiceApiError,
    {
        let response = self
            .client
            // Method doesn't implement Copy
            .request(parts.method.clone(), &parts.url_with_qs)
            // body is Bytes which can be cheaply cloned
            .body(parts.body.clone())
            .send()
            .await
            .map_err(CommonError::from)?;

        if response.status().is_success() {
            // Uncomment for debugging
            // let text = response.text().await?;
            // println!("Response: {}", text);
            // serde_json::from_str(&text).map_err(|e| e.into())

            // Deserialize into Ok variant, return Ok(json)
            response
                .json::<T>()
                .await
                .map_err(CommonError::from)
                .map_err(E::from)
        } else {
            // Deserialize into Err variant, return Err(json)
            let error_response = response
                .json::<ErrorResponse>()
                .await
                .map_err(CommonError::from)?;
            Err(E::from(error_response))
        }
    }
}
