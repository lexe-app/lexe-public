use std::{
    convert::Infallible,
    future::Future,
    net::{SocketAddr, TcpListener},
    time::Duration,
};

use anyhow::Context;
use bytes::Bytes;
use futures::future::BoxFuture;
use reqwest::IntoUrl;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time;
use tracing::{
    debug, error, field, info, info_span, span, warn, Instrument, Span,
};
use warp::{
    filters::BoxedFilter,
    http::{
        header::{HeaderValue, CONTENT_TYPE},
        response::Response,
        status::StatusCode,
        Method,
    },
    hyper::Body,
    Filter, Rejection,
};

use crate::{
    api::error::{
        ErrorCode, ErrorResponse, RestClientError, RestClientErrorKind,
        ServiceApiError, ToHttpStatus,
    },
    backoff,
    byte_str::ByteStr,
    ed25519,
    shutdown::ShutdownChannel,
    task::LxTask,
};

/// The CONTENT-TYPE header for signed BCS-serialized structs.
pub static CONTENT_TYPE_ED25519_BCS: HeaderValue =
    HeaderValue::from_static("application/ed25519-bcs");

// Default parameters
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The maximum time [`hyper::Server`] can take to gracefully shut down.
pub const HYPER_TIMEOUT: Duration = Duration::from_secs(3);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

/// Helper to serve a set of [`warp`] routes given a graceful shutdown
/// [`Future`], an existing std [`TcpListener`], the name of the task, and a
/// [`tracing::Span`]. Be sure to include `parent: None` when building the span
/// if you wish to prevent the API task from inheriting the parent [span label].
pub fn serve_routes_with_listener_and_shutdown<F>(
    routes: BoxedFilter<(Response<Body>,)>,
    graceful_shutdown_fut: F,
    listener: TcpListener,
    task_name: &'static str,
    span: Span,
) -> anyhow::Result<(LxTask<()>, SocketAddr)>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_routes_with_listener_and_shutdown_boxed(
        routes,
        Box::pin(graceful_shutdown_fut),
        listener,
        task_name,
        span,
    )
}

// Reduce some code bloat by boxing the warp routes and shutdown future.
fn serve_routes_with_listener_and_shutdown_boxed(
    routes: BoxedFilter<(Response<Body>,)>,
    graceful_shutdown_fut: BoxFuture<'static, ()>,
    listener: TcpListener,
    task_name: &'static str,
    span: Span,
) -> anyhow::Result<(LxTask<()>, SocketAddr)> {
    let api_service = warp::service(routes.with(trace_requests(span.id())));
    let make_service = hyper::service::make_service_fn(move |_| {
        let api_service_clone = api_service.clone();
        async move { Ok::<_, Infallible>(api_service_clone) }
    });
    let server = hyper::Server::from_tcp(listener)
        .context("Could not create hyper Server")?
        .serve(make_service);
    let socket_addr = server.local_addr();
    // Instead of giving the graceful shutdown future to hyper directly, we
    // let the spawned task wait on it so that we can enforce a hyper timeout.
    let shutdown = ShutdownChannel::new();
    let graceful_server =
        server.with_graceful_shutdown(shutdown.clone().recv_owned());
    let task = LxTask::spawn_named_with_span(task_name, span, async move {
        tokio::pin!(graceful_server);
        tokio::select! {
            () = graceful_shutdown_fut => (),
            _ = &mut graceful_server => return error!("Server exited early"),
        }
        info!("Initiating hyper server graceful shutdown");
        shutdown.send();
        match time::timeout(HYPER_TIMEOUT, graceful_server).await {
            Ok(Ok(())) => debug!("Hyper server shutdown success"),
            Ok(Err(e)) => warn!("Hyper server returned error: {e:#}"),
            Err(_) => warn!("Hyper server timed out during shutdown"),
        }
    });
    Ok((task, socket_addr))
}

/// Adds [`tracing`] to all requests.
///
/// * Wraps all requests in a [`tracing::Span`] for the duration of the request.
/// * `debug!` logs when the request initally enters the warp router.
/// * `info!` logs when the request completes and the response is generated.
///   Includes info like response status, handling time, and error messages.
///
/// It manually takes an (optional) [`span::Id`] for the parent span. This is
/// definitely a bit awkward, but it seems to work for now. The hyper service
/// does request handling on freshly spawned tasks, so it's a bit difficult to
/// get our warp routes to pick up the correct parent span without just manually
/// passing the right id over.
///
/// ## Usage
///
/// ```ignore
/// let api_span = info!(parent: None, "(api)");
///
/// let routes = node_proxy.or(backend_apis).or(app_gateway_api);
///
/// routes.with(rest::trace_requests(api_span.id())).boxed()
/// ```
pub fn trace_requests(
    parent_span_id: Option<span::Id>,
) -> warp::trace::Trace<impl Fn(warp::trace::Info<'_>) -> Span + Clone> {
    warp::trace::trace(move |req_info| {
        let url = req_info
            .uri()
            .path_and_query()
            .map(|url| url.as_str())
            .unwrap_or("/");
        info_span!(
            target: "http",
            parent: parent_span_id.clone(),
            "(http)(srv)",
            method = %req_info.method(),
            url = %url,
            version = ?req_info.version(),
        )
    })
}

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
///     .and(warp::query::<GetByUserPk>())
///     .and(inject::user_pk(user_pk))
///     .then(runner::status)
///     .map(rest::into_response);
/// ```
pub fn into_response<T: Serialize, E: ToHttpStatus + Into<ErrorResponse>>(
    reply_res: Result<T, E>,
) -> Response<Body> {
    match reply_res {
        Ok(data) => build_json_response(StatusCode::OK, &data),
        Err(err) => build_json_response(err.to_http_status(), &err.into()),
    }
}

/// Like [`into_response`], but converts `T` into [`Response<Body>`]. This fn
/// should be used for the same reasons that [`into_response`] is used, but
/// applies only to *infallible* handlers.
///
/// ## Usage
///
/// ```ignore
/// let list_channels = warp::path("list_channels")
///     .and(warp::get())
///     .and(inject::channel_manager(channel_manager.clone()))
///     .map(lexe_ln::command::list_channels)
///     .map(rest::into_succ_response);
/// ```
pub fn into_succ_response<T: Serialize>(data: T) -> Response<Body> {
    build_json_response(StatusCode::OK, &data)
}

/// Like [`into_response`], but you pass a successful, pre-rendered json
/// response instead of serializing on-the-spot. Can be useful if a response is
/// already cached and serialized.
///
/// ## Usage
///
/// ```ignore
/// fn handler() -> Result<ByteStr, Error> {
///     Ok(ByteStr::from_static(r#"{ "foo": 123, "bar": "asdf" }"#))
/// }
///
/// let route = warp::get()
///     .map(handler)
///     .map(rest::prerendered_json_into_response);
/// ```
pub fn prerendered_json_into_response<E: ToHttpStatus + Into<ErrorResponse>>(
    reply_res: Result<ByteStr, E>,
) -> Response<Body> {
    match reply_res {
        Ok(data) => build_json_response_inner(StatusCode::OK, Ok(data.into())),
        Err(err) => build_json_response(err.to_http_status(), &err.into()),
    }
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
///     .map(rest::into_response);
///
/// root.or(foo)
///     // recover the `GatewayApiError` from above and return standard json
///     // error response
///     .recover(recover_error_response::<GatewayApiError>)
/// ```
pub async fn recover_error_response<
    E: Clone + ToHttpStatus + Into<ErrorResponse> + warp::reject::Reject + 'static,
>(
    err: Rejection,
) -> Result<Response<Body>, Rejection> {
    if let Some(err) = err.find::<E>() {
        let status = err.to_http_status();
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
    status: StatusCode,
    data: &T,
) -> Response<Body> {
    build_json_response_inner(status, serde_json::to_vec(data).map(Bytes::from))
}

fn build_json_response_inner(
    mut status: StatusCode,
    maybe_json: Result<Bytes, serde_json::Error>,
) -> Response<Body> {
    let body = maybe_json.map(Body::from).unwrap_or_else(|e| {
        error!(target: "http", "Couldn't serialize response: {e:#}");
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

/// Converts the concrete, non-generic Rest response result to the specific
/// API's result type.
///
/// On success, this json deserializes the response body. On error, this
/// converts the generic [`ErrorResponse`] or [`RestClientError`] into the
/// specific API error type, like
/// [`BackendApiError`](crate::api::error::BackendApiError)
fn convert_rest_response<T, E>(
    response: Result<Result<Bytes, ErrorResponse>, RestClientError>,
) -> Result<T, E>
where
    T: DeserializeOwned,
    E: ServiceApiError,
{
    match response {
        Ok(Ok(bytes)) =>
            Ok(serde_json::from_slice::<T>(&bytes).map_err(|err| {
                let kind = RestClientErrorKind::Decode;
                let msg = format!("Failed to deser response as json: {err:#}");
                RestClientError::new(kind, msg)
            })?),
        Ok(Err(err_api)) => Err(E::from(err_api)),
        Err(err_client) => Err(E::from(err_client)),
    }
}

/// A generic RestClient. [`reqwest::Client`] holds an [`Arc`] internally, so
/// likewise, [`RestClient`] can be cloned and used directly, without [`Arc`].
///
/// [`Arc`]: std::sync::Arc
#[derive(Clone)]
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

    fn request_span(req: &reqwest::Request) -> tracing::Span {
        info_span!(
            target: "http",
            "(http)(cli)",
            method = %req.method(),
            url = %req.url(),
            // the "attempts left" is set in the span later on
            attempts_left = field::Empty,
        )
    }

    /// Sends the built HTTP request once. Tries to JSON deserialize the
    /// response body to `T`.
    pub async fn send<T, E>(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<T, E>
    where
        T: DeserializeOwned,
        E: ServiceApiError,
    {
        let request = request_builder.build().map_err(RestClientError::from)?;
        let span = Self::request_span(&request);
        let response = self.send_inner(request).instrument(span).await;
        convert_rest_response(response)
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
        E: ServiceApiError,
    {
        let request = request_builder.build().map_err(RestClientError::from)?;
        let span = Self::request_span(&request);
        let response = self
            .send_with_retries_inner(request, retries, stop_codes)
            .instrument(span)
            .await;
        convert_rest_response(response)
    }

    // the `send_inner` and `send_with_retries_inner` intentionally use zero
    // generics in their function signatures to minimize code bloat.

    async fn send_with_retries_inner(
        &self,
        request: reqwest::Request,
        retries: usize,
        stop_codes: &[ErrorCode],
    ) -> Result<Result<Bytes, ErrorResponse>, RestClientError> {
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
            match self.send_inner(request_clone).await {
                Ok(Ok(bytes)) => return Ok(Ok(bytes)),
                Ok(Err(err_api)) =>
                    if stop_codes.contains(&err_api.code) {
                        return Ok(Err(err_api));
                    },
                Err(err_client) => {
                    if stop_codes.contains(&err_client.to_code()) {
                        return Err(err_client);
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

        self.send_inner(request.take().unwrap()).await
    }

    async fn send_inner(
        &self,
        mut request: reqwest::Request,
    ) -> Result<Result<Bytes, ErrorResponse>, RestClientError> {
        let start = tokio::time::Instant::now().into_std();
        debug!(target: "http", "New (outbound) Sending request");

        // set default timeout if unset
        let timeout = request.timeout_mut();
        if timeout.is_none() {
            *timeout = Some(API_REQUEST_TIMEOUT);
        }

        // send the request, await the response headers
        let resp = self.client.execute(request).await.inspect_err(|err| {
            let time = start.elapsed();
            warn!(
                target: "http",
                ?time,
                "Done (err)(sending) Error sending request: {err:#}"
            );
        })?;

        // add the response http status to the current request span
        let status = resp.status().as_u16();

        if resp.status().is_success() {
            // success => await response body
            let bytes = resp.bytes().await.inspect_err(|err| {
                let time = start.elapsed();
                warn!(
                    target: "http",
                    %status,
                    ?time,
                    "Done (err)(receiving) Couldn't receive success response body: {err:#}",
                );
            })?;

            let time = start.elapsed();
            info!(target: "http", %status, ?time, "Done (success)");
            Ok(Ok(bytes))
        } else {
            // http error => await response json and convert to ErrorResponse
            let err = resp.json::<ErrorResponse>().await.inspect_err(|err| {
                let time = start.elapsed();
                warn!(
                    target: "http",
                    %status,
                    ?time,
                    "Done (err)(receiving) Couldn't receive ErrorResponse json: {err:#}",
                );
            })?;

            let time = start.elapsed();
            warn!(
                target: "http",
                %status,
                ?time,
                err_code = %err.code,
                err_msg = %err.msg,
                "Done (err)(response) Server returned error response",
            );
            Ok(Err(err))
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
