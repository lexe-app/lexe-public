use std::time::Duration;

use bytes::Bytes;
use http::{
    header::{HeaderValue, CONTENT_TYPE},
    Method,
};
use reqwest::IntoUrl;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info, warn, Instrument};

use super::trace::TraceId;
use crate::{
    api::{
        error::{
            ApiError, CommonApiError, CommonErrorKind, ErrorCode, ErrorResponse,
        },
        trace::{self, DisplayMs},
    },
    backoff, ed25519,
};

/// The CONTENT-TYPE header for signed BCS-serialized structs.
pub static CONTENT_TYPE_ED25519_BCS: HeaderValue =
    HeaderValue::from_static("application/ed25519-bcs");

// Default parameters
pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

/// A generic RestClient which conforms to Lexe's API.
#[derive(Clone)]
pub struct RestClient {
    client: reqwest::Client,
    /// The process that this [`RestClient`] is being called from, e.g. "app"
    from: &'static str,
    /// The process that this [`RestClient`] is calling, e.g. "node-run"
    to: &'static str,
}

impl RestClient {
    /// Builds a new [`RestClient`] with the given TLS config.
    ///
    /// The `from` and `to` fields should specify the client and server
    /// components of the API trait that this [`RestClient`] is used for.
    /// The [`RestClient`] will log both fields so that requests from this
    /// client can be differentiated from those made by other clients in the
    /// same process, and propagate the `from` field to the server via the user
    /// agent header so that servers can identify requesting clients.
    ///
    /// ```
    /// # use common::api::rest::RestClient;
    /// # use http::header::HeaderValue;
    /// let backend_api = RestClient::new("node", "backend");
    /// let runner_api = RestClient::new("node", "runner");
    /// ```
    // TODO(max): Rename to just `new` after TLS is implemented everywhere
    pub fn new_tls(
        from: &'static str,
        to: &'static str,
        tls_config: rustls::ClientConfig,
    ) -> Self {
        let client = Self::client_builder(from)
            .use_preconfigured_tls(tls_config)
            .build()
            .expect("Failed to build reqwest Client");
        Self { client, from, to }
    }

    /// The `from` and `to` fields should specify the client and server
    /// components of the API trait that this [`RestClient`] is used for.
    /// The [`RestClient`] will log both fields so that requests from this
    /// client can be differentiated from those made by other clients in the
    /// same process, and propagate the `from` field to the server via the user
    /// agent header so that servers can identify requesting clients.
    ///
    /// ```
    /// # use common::api::rest::RestClient;
    /// # use http::header::HeaderValue;
    /// let backend_api = RestClient::new("node", "backend");
    /// let runner_api = RestClient::new("node", "runner");
    /// ```
    // TODO(max): Rename to `new_insecure` after TLS is implemented everywhere
    pub fn new(from: &'static str, to: &'static str) -> Self {
        let client = Self::client_builder(from)
            .build()
            .expect("Failed to build reqwest Client");
        Self { client, from, to }
    }

    /// Get a [`reqwest::ClientBuilder`] with some defaults set.
    pub fn client_builder(from: &'static str) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .user_agent(from)
            .timeout(API_REQUEST_TIMEOUT)
    }

    /// Construct a [`RestClient`] from a [`reqwest::Client`].
    pub fn from_inner(
        client: reqwest::Client,
        from: &'static str,
        to: &'static str,
    ) -> Self {
        Self { client, from, to }
    }

    // --- RequestBuilder helpers --- //

    /// Return a clean slate [`reqwest::RequestBuilder`] for non-standard
    /// requests. Otherwise prefer to use the ready-made `get`, `post`, ..., etc
    /// helpers.
    pub fn builder(
        &self,
        method: Method,
        url: impl IntoUrl,
    ) -> reqwest::RequestBuilder {
        self.client.request(method, url)
    }

    #[inline]
    pub fn get<U, T>(&self, url: U, data: &T) -> reqwest::RequestBuilder
    where
        U: IntoUrl,
        T: Serialize + ?Sized,
    {
        self.builder(GET, url).query(data)
    }

    #[inline]
    pub fn post<U, T>(&self, url: U, data: &T) -> reqwest::RequestBuilder
    where
        U: IntoUrl,
        T: Serialize + ?Sized,
    {
        self.builder(POST, url).json(data)
    }

    #[inline]
    pub fn put<U, T>(&self, url: U, data: &T) -> reqwest::RequestBuilder
    where
        U: IntoUrl,
        T: Serialize + ?Sized,
    {
        self.builder(PUT, url).json(data)
    }

    #[inline]
    pub fn delete<U, T>(&self, url: U, data: &T) -> reqwest::RequestBuilder
    where
        U: IntoUrl,
        T: Serialize + ?Sized,
    {
        self.builder(DELETE, url).json(data)
    }

    // --- Request send/recv --- //

    /// Sends the built HTTP request once. Tries to JSON deserialize the
    /// response body to `T`.
    pub async fn send<T, E>(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<T, E>
    where
        T: DeserializeOwned,
        E: ApiError,
    {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, self.from, self.to);
        let response = self
            .send_inner(request, &trace_id)
            .instrument(request_span)
            .await;
        Self::convert_rest_response(response)
    }

    /// Sends the built HTTP request, retrying up to `retries` times. Tries to
    /// JSON deserialize the response body to `T`.
    ///
    /// If one of the request attempts yields an error code in `stop_codes`, we
    /// will immediately stop retrying and return that error.
    ///
    /// See also: [`RestClient::send`]
    pub async fn send_with_retries<T, E>(
        &self,
        request_builder: reqwest::RequestBuilder,
        retries: usize,
        stop_codes: &[ErrorCode],
    ) -> Result<T, E>
    where
        T: DeserializeOwned,
        E: ApiError,
    {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, self.from, self.to);
        let response = self
            .send_with_retries_inner(request, retries, stop_codes, &trace_id)
            .instrument(request_span)
            .await;
        Self::convert_rest_response(response)
    }

    // the `send_inner` and `send_with_retries_inner` intentionally use zero
    // generics in their function signatures to minimize code bloat.

    async fn send_with_retries_inner(
        &self,
        request: reqwest::Request,
        retries: usize,
        stop_codes: &[ErrorCode],
        trace_id: &TraceId,
    ) -> Result<Result<Bytes, ErrorResponse>, CommonApiError> {
        let mut backoff_durations = backoff::get_backoff_iter();
        let mut attempts_left = retries + 1;

        let mut request = Some(request);

        // Do the 'retries' first.
        for _ in 0..retries {
            tracing::Span::current().record("attempts_left", attempts_left);

            // clone the request. the request body is cheaply cloneable. the
            // headers and url are not :'(
            let maybe_request_clone = request
                .as_ref()
                .expect(
                    "This should never happen; we only take() the original \
                     request on the last attempt",
                )
                .try_clone();

            let request_clone = match maybe_request_clone {
                Some(request_clone) => request_clone,
                // We only get None if the request body is streamed and not set
                // up front. In this case, we can't send more than once.
                None => break,
            };

            // send the request and look for any error codes in the response
            // that we should bail on and stop retrying.
            match self.send_inner(request_clone, trace_id).await {
                Ok(Ok(bytes)) => return Ok(Ok(bytes)),
                Ok(Err(api_error)) =>
                    if stop_codes.contains(&api_error.code) {
                        return Ok(Err(api_error));
                    },
                Err(common_error) => {
                    if stop_codes.contains(&common_error.to_code()) {
                        return Err(common_error);
                    }
                }
            }

            // sleep for a bit before next retry
            tokio::time::sleep(backoff_durations.next().unwrap()).await;
            attempts_left -= 1;
        }

        // We ran out of retries; return the result of the 'main' attempt.
        assert_eq!(attempts_left, 1);
        tracing::Span::current().record("attempts_left", attempts_left);

        self.send_inner(request.take().unwrap(), trace_id).await
    }

    async fn send_inner(
        &self,
        mut request: reqwest::Request,
        trace_id: &TraceId,
    ) -> Result<Result<Bytes, ErrorResponse>, CommonApiError> {
        let start = tokio::time::Instant::now().into_std();
        // This message should mirror `LxOnRequest`.
        debug!(target: trace::TARGET, "New client request");

        // Add the trace id header to the request.
        match request.headers_mut().try_insert(
            trace::TRACE_ID_HEADER_NAME.clone(),
            trace_id.to_header_value(),
        ) {
            Ok(None) => (),
            Ok(Some(_)) => warn!(target: trace::TARGET, "Trace id existed?"),
            Err(e) => warn!(target: trace::TARGET, "Header map full?: {e:#}"),
        }

        // send the request, await the response headers
        let resp = self.client.execute(request).await.inspect_err(|e| {
            let req_time = DisplayMs(start.elapsed());
            warn!(
                target: trace::TARGET,
                %req_time,
                "Done (error)(sending) Error sending request: {e:#}"
            );
        })?;

        // add the response http status to the current request span
        let status = resp.status().as_u16();

        if resp.status().is_success() {
            // success => await response body
            let bytes = resp.bytes().await.inspect_err(|e| {
                let req_time = DisplayMs(start.elapsed());
                warn!(
                    target: trace::TARGET,
                    %req_time,
                    %status,
                    "Done (error)(receiving) \
                     Couldn't receive success response body: {e:#}",
                );
            })?;

            let req_time = DisplayMs(start.elapsed());
            info!(target: trace::TARGET, %req_time, %status, "Done (success)");
            Ok(Ok(bytes))
        } else {
            // http error => await response json and convert to ErrorResponse
            let error =
                resp.json::<ErrorResponse>().await.inspect_err(|e| {
                    let req_time = DisplayMs(start.elapsed());
                    warn!(
                        target: trace::TARGET,
                        %req_time,
                        %status,
                        "Done (error)(receiving) \
                         Couldn't receive ErrorResponse: {e:#}",
                    );
                })?;

            let req_time = DisplayMs(start.elapsed());
            warn!(
                target: trace::TARGET,
                %req_time,
                %status,
                error_code = %error.code,
                error_msg = %error.msg,
                "Done (error)(response) Server returned error response",
            );
            Ok(Err(error))
        }
    }

    /// Converts the concrete, non-generic Rest response result to the specific
    /// API's result type.
    ///
    /// On success, this json deserializes the response body. On error, this
    /// converts the generic [`ErrorResponse`] or [`CommonApiError`] into the
    /// specific API error type, like [`BackendApiError`].
    ///
    /// [`BackendApiError`]: crate::api::error::BackendApiError
    fn convert_rest_response<T, E>(
        response: Result<Result<Bytes, ErrorResponse>, CommonApiError>,
    ) -> Result<T, E>
    where
        T: DeserializeOwned,
        E: ApiError,
    {
        match response {
            Ok(Ok(bytes)) =>
                Ok(serde_json::from_slice::<T>(&bytes).map_err(|err| {
                    let kind = CommonErrorKind::Decode;
                    let msg =
                        format!("Failed to deser response as json: {err:#}");
                    CommonApiError::new(kind, msg)
                })?),
            Ok(Err(err_api)) => Err(E::from(err_api)),
            Err(err_client) => Err(E::from(err_client)),
        }
    }
}

// -- impl RequestBuilderExt -- //

/// Extension trait on [`reqwest::RequestBuilder`] for easily modifying requests
/// as they're constructed.
pub trait RequestBuilderExt: Sized {
    /// Set the request body to a [`ed25519::Signed<T>`] serialized to BCS with
    /// corresponding content type header.
    fn signed_bcs<T>(
        self,
        signed_bcs: ed25519::Signed<T>,
    ) -> Result<Self, bcs::Error>
    where
        T: ed25519::Signable + Serialize;
}

impl RequestBuilderExt for reqwest::RequestBuilder {
    fn signed_bcs<T>(
        self,
        signed_bcs: ed25519::Signed<T>,
    ) -> Result<Self, bcs::Error>
    where
        T: ed25519::Signable + Serialize,
    {
        Ok(self
            .header(CONTENT_TYPE, CONTENT_TYPE_ED25519_BCS.clone())
            .body(signed_bcs.serialize()?))
    }
}
