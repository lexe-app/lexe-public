use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

use bytes::Bytes;
use common::{ed25519, time::DisplayMs};
use http::{
    Method,
    header::{CONTENT_TYPE, HeaderValue},
};
use lexe_api_core::error::{
    ApiError, CommonApiError, CommonErrorKind, ErrorCode, ErrorResponse,
};
use lexe_std::backoff;
use lightning::util::ser::Writeable;
use reqwest::IntoUrl;
use serde::{Serialize, de::DeserializeOwned};
use tracing::{Instrument, debug, warn};

use crate::{trace, trace::TraceId};

/// The CONTENT-TYPE header for signed BCS-serialized structs.
pub static CONTENT_TYPE_ED25519_BCS: HeaderValue =
    HeaderValue::from_static("application/ed25519-bcs");

// Apparently it takes >15s to open a channel with an external peer.
pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    from: Cow<'static, str>,
    /// The process that this [`RestClient`] is calling, e.g. "node-run"
    to: &'static str,
}

impl RestClient {
    /// Builds a new [`RestClient`] with the given TLS config and safe defaults.
    ///
    /// The `from` and `to` fields should succinctly specify the client and
    /// server components of the API trait that this [`RestClient`] is used for,
    /// e.g. `from`="app", `to`="node-run" or `from`="node", `to`="backend".
    /// The [`RestClient`] will log both fields so that requests from this
    /// client can be differentiated from those made by other clients in the
    /// same process, and propagate the `from` field to the server via the user
    /// agent header so that servers can identify requesting clients.
    pub fn new(
        from: impl Into<Cow<'static, str>>,
        to: &'static str,
        tls_config: rustls::ClientConfig,
    ) -> Self {
        fn inner(
            from: Cow<'static, str>,
            to: &'static str,
            tls_config: rustls::ClientConfig,
        ) -> RestClient {
            let client = RestClient::client_builder(&from)
                .use_preconfigured_tls(tls_config)
                .https_only(true)
                .build()
                .expect("Failed to build reqwest Client");
            RestClient { client, from, to }
        }
        inner(from.into(), to, tls_config)
    }

    /// [`RestClient::new`] but without TLS.
    /// This should only be used for non-security-critical endpoints.
    pub fn new_insecure(
        from: impl Into<Cow<'static, str>>,
        to: &'static str,
    ) -> Self {
        fn inner(from: Cow<'static, str>, to: &'static str) -> RestClient {
            let client = RestClient::client_builder(&from)
                .https_only(false)
                .build()
                .expect("Failed to build reqwest Client");
            RestClient { client, from, to }
        }
        inner(from.into(), to)
    }

    /// Get a [`reqwest::ClientBuilder`] with some defaults set.
    /// NOTE that for safety, `https_only` is set to `true`, but you can
    /// override it if needed.
    pub fn client_builder(from: impl AsRef<str>) -> reqwest::ClientBuilder {
        fn inner(from: &str) -> reqwest::ClientBuilder {
            reqwest::Client::builder()
                .user_agent(from)
                .https_only(true)
                .timeout(API_REQUEST_TIMEOUT)
        }
        inner(from.as_ref())
    }

    /// Construct a [`RestClient`] from a [`reqwest::Client`].
    pub fn from_inner(
        client: reqwest::Client,
        from: impl Into<Cow<'static, str>>,
        to: &'static str,
    ) -> Self {
        Self {
            client,
            from: from.into(),
            to,
        }
    }

    #[inline]
    pub fn user_agent(&self) -> &Cow<'static, str> {
        &self.from
    }

    // --- RequestBuilder helpers --- //

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

    /// Serializes a LDK [`Writeable`] object into the request body.
    #[inline]
    pub fn serialize_ldk_writeable<U, W>(
        &self,
        method: Method,
        url: U,
        data: &W,
    ) -> reqwest::RequestBuilder
    where
        U: IntoUrl,
        W: Writeable,
    {
        let bytes = {
            let mut buf = Vec::new();
            data.write(&mut buf)
                .expect("Serializing into in-memory buf shouldn't fail");
            Bytes::from(buf)
        };
        self.builder(method, url).body(bytes)
    }

    /// A clean slate [`reqwest::RequestBuilder`] for non-standard requests.
    /// Otherwise prefer to use the ready-made `get`, `post`, ..., etc helpers.
    pub fn builder(
        &self,
        method: Method,
        url: impl IntoUrl,
    ) -> reqwest::RequestBuilder {
        self.client.request(method, url)
    }

    // --- Request send/recv --- //

    /// Sends the built HTTP request.
    /// Tries to JSON deserialize the response body to `T`.
    pub async fn send<T: DeserializeOwned, E: ApiError>(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<T, E> {
        let bytes = self.send_no_deserialize::<E>(request_builder).await?;
        Self::json_deserialize(bytes)
    }

    /// Sends the HTTP request, but *doesn't* JSON-deserialize the response.
    pub async fn send_no_deserialize<E: ApiError>(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<Bytes, E> {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, &self.from, self.to);
        let response = self
            .send_inner(request, &trace_id)
            .instrument(request_span)
            .await;
        let res = match response {
            Ok(Ok(resp)) => resp.read_bytes().await.map(Ok),
            Ok(Err(api_error)) => Ok(Err(api_error)),
            Err(common_error) => Err(common_error),
        };
        Self::map_response_errors::<Bytes, E>(res)
    }

    /// Sends the HTTP request, but returns a [`StreamBody`] that yields
    /// [`Bytes`] chunks as they arrive.
    pub async fn send_and_stream_response<E: ApiError>(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<StreamBody, E> {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, &self.from, self.to);
        let response = self
            .send_inner(request, &trace_id)
            .instrument(request_span)
            .await;
        Self::map_response_errors::<SuccessResponse, E>(response)
            .map(|resp| resp.into_stream_body())
    }

    /// Sends the built HTTP request, retrying up to `retries` times. Tries to
    /// JSON deserialize the response body to `T`.
    ///
    /// If one of the request attempts yields an error code in `stop_codes`, we
    /// will immediately stop retrying and return that error.
    ///
    /// See also: [`RestClient::send`]
    pub async fn send_with_retries<T: DeserializeOwned, E: ApiError>(
        &self,
        request_builder: reqwest::RequestBuilder,
        retries: usize,
        stop_codes: &[ErrorCode],
    ) -> Result<T, E> {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, &self.from, self.to);
        let response = self
            .send_with_retries_inner(request, retries, stop_codes, &trace_id)
            .instrument(request_span)
            .await;
        let bytes = Self::map_response_errors::<Bytes, E>(response)?;
        Self::json_deserialize(bytes)
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
                Ok(Ok(resp)) => match resp.read_bytes().await {
                    Ok(bytes) => {
                        return Ok(Ok(bytes));
                    }
                    Err(common_error) => {
                        if stop_codes.contains(&common_error.to_code()) {
                            return Err(common_error);
                        }
                    }
                },
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

        let resp = self.send_inner(request.take().unwrap(), trace_id).await?;
        match resp {
            Ok(resp_succ) => resp_succ.read_bytes().await.map(Ok),
            Err(api_error) => Ok(Err(api_error)),
        }
    }

    async fn send_inner(
        &self,
        mut request: reqwest::Request,
        trace_id: &TraceId,
    ) -> Result<Result<SuccessResponse, ErrorResponse>, CommonApiError> {
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
            Ok(Ok(SuccessResponse { resp, start }))
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

    /// Converts the [`Result<Result<T, ErrorResponse>, CommonApiError>`]
    /// returned by [`Self::send_inner`] to [`Result<T, E>`].
    fn map_response_errors<T, E: ApiError>(
        response: Result<Result<T, ErrorResponse>, CommonApiError>,
    ) -> Result<T, E> {
        match response {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(err_api)) => Err(E::from(err_api)),
            Err(err_client) => Err(E::from(err_client)),
        }
    }

    /// JSON-deserializes the REST response bytes.
    fn json_deserialize<T: DeserializeOwned, E: ApiError>(
        bytes: Bytes,
    ) -> Result<T, E> {
        serde_json::from_slice::<T>(&bytes)
            .map_err(|err| {
                let kind = CommonErrorKind::Decode;
                let mut msg = format!("JSON deserialization failed: {err:#}");

                // If we're in debug, append the response str to the error msg.
                // TODO(max): Try to find a way to do this safely in prod.
                if cfg!(any(debug_assertions, test, feature = "test-utils")) {
                    let resp_msg = String::from_utf8_lossy(&bytes);
                    msg.push_str(&format!(": '{resp_msg}'"));
                }

                CommonApiError::new(kind, msg)
            })
            .map_err(E::from)
    }
}

// -- impl SuccessResponse -- //

/// A successful [`reqwest::Response`], though we haven't read the body yet.
struct SuccessResponse {
    resp: reqwest::Response,
    start: Instant,
}

impl SuccessResponse {
    /// Convert into a streaming response body.
    fn into_stream_body(self) -> StreamBody {
        StreamBody {
            resp: self.resp,
            start: self.start,
        }
    }

    /// Read the successful response body into a single raw [`Bytes`].
    async fn read_bytes(self) -> Result<Bytes, CommonApiError> {
        let status = self.resp.status().as_u16();
        let bytes = self.resp.bytes().await.inspect_err(|e| {
            let req_time = DisplayMs(self.start.elapsed());
            warn!(
                target: trace::TARGET,
                %req_time,
                %status,
                "Done (error)(receiving) \
                 Couldn't receive response body: {e:#}",
            );
        })?;

        let req_time = DisplayMs(self.start.elapsed());
        // NOTE: This client request log can be at INFO.
        // It's cluttering our logs though, so we're suppressing.
        debug!(target: trace::TARGET, %req_time, %status, "Done (success)");
        Ok(bytes)
    }
}

// -- impl StreamResponse -- //

/// A streaming response body which yields chunks of the body as raw [`Bytes`]
/// as they arrive.
pub struct StreamBody {
    resp: reqwest::Response,
    start: Instant,
}

impl StreamBody {
    /// Stream a chunk of the response body. Returns `Ok(None)` when the stream
    /// is complete.
    pub async fn next_chunk(
        &mut self,
    ) -> Result<Option<Bytes>, CommonApiError> {
        match self.resp.chunk().await {
            Ok(Some(chunk)) => Ok(Some(chunk)),
            Ok(None) => {
                // Done, log how long it took.
                let status = self.resp.status().as_u16();
                let req_time = DisplayMs(self.start.elapsed());
                debug!(target: trace::TARGET, %req_time, %status, "Done (success)");
                Ok(None)
            }
            Err(e) => {
                // Error receiving next chunk.
                let status = self.resp.status().as_u16();
                let req_time = DisplayMs(self.start.elapsed());
                warn!(
                    target: trace::TARGET,
                    %req_time,
                    %status,
                    "Done (error)(receiving) \
                     Couldn't receive streaming response chunk: {e:#}",
                );
                Err(CommonApiError::from(e))
            }
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
        signed_bcs: &ed25519::Signed<&T>,
    ) -> Result<Self, bcs::Error>
    where
        T: ed25519::Signable + Serialize;
}

impl RequestBuilderExt for reqwest::RequestBuilder {
    fn signed_bcs<T>(
        self,
        signed_bcs: &ed25519::Signed<&T>,
    ) -> Result<Self, bcs::Error>
    where
        T: ed25519::Signable + Serialize,
    {
        let bytes = signed_bcs.serialize()?;
        let request = self
            .header(CONTENT_TYPE, CONTENT_TYPE_ED25519_BCS.clone())
            .body(bytes);
        Ok(request)
    }
}
