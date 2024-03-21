use std::{
    convert::Infallible,
    future::Future,
    net::{SocketAddr, TcpListener},
    time::Duration,
};

use anyhow::Context;
use bytes::Bytes;
use futures::future::BoxFuture;
use http::{
    header::{HeaderValue, CONTENT_TYPE},
    Method,
};
use http_old::{
    header::{HeaderValue as OldHeaderValue, CONTENT_TYPE as OLD_CONTENT_TYPE},
    response::Response as OldResponse,
    status::StatusCode as OldStatusCode,
};
use reqwest::IntoUrl;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time;
use tracing::{debug, error, info, info_span, span, warn, Instrument, Span};
use warp::{filters::BoxedFilter, hyper::Body, Filter, Rejection};

use super::trace::TraceId;
use crate::{
    api::{
        error::{
            ApiError, CommonApiError, CommonErrorKind, ErrorCode,
            ErrorResponse, ToHttpStatus,
        },
        trace::{self, DisplayMs},
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
pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The maximum time [`hyper_old::Server`] can take to gracefully shut down.
pub const HYPER_TIMEOUT: Duration = Duration::from_secs(3);

// Avoid `Method::` prefix. Associated constants can't be imported
pub const GET: Method = Method::GET;
pub const PUT: Method = Method::PUT;
pub const POST: Method = Method::POST;
pub const DELETE: Method = Method::DELETE;

/// Builds a service future given the warp routes, TLS config, and other info.
/// The resulting `impl Future<Output = ()>` can be spawned into a [`LxTask`].
// NOTE: We intentionally avoid generics to avoid code bloat.
// NOTE: We return a future and don't spawn it into a task because some of our
// orchestration code benefits from using futures (as opposed to tasks) as its
// basic unit, so as to reduce indirection from multiple layers of tasks.
pub fn build_service_fut(
    routes: BoxedFilter<(OldResponse<Body>,)>,
    tls_config: rustls::ServerConfig,
    // TODO(max): This needs to be a TcpListener, since breaking the LSP <->
    // Runner codependency requires binding the runner's TcpListener first.
    // TODO(max): Remove the SocketAddr from return type once complete
    bind_addr: SocketAddr,
    api_span: &Span,
    // The server will listen on this channel for a graceful shutdown signal.
    shutdown: ShutdownChannel,
) -> (SocketAddr, BoxFuture<'static, ()>) {
    let instrumented_routes = routes.with(trace_requests(api_span.id()));
    // TODO(max): This server needs backpressure
    let server = warp::serve(instrumented_routes);
    let tls_server = server.tls().preconfigured_tls(tls_config);
    let (addr, service_fut) = tls_server
        // TODO(max): Enforce timeout on webserver shutdown with `HYPER_TIMEOUT`
        .bind_with_graceful_shutdown(bind_addr, shutdown.recv_owned());
    let boxed_service_fut = Box::pin(service_fut);
    (addr, boxed_service_fut)
}

/// Helper to serve a set of [`warp`] routes given a graceful shutdown
/// [`Future`], an existing std [`TcpListener`], the name of the task, and a
/// [`tracing::Span`]. Be sure to include `parent: None` when building the span
/// if you wish to prevent the API task from inheriting the parent [span label].
// TODO(max): Remove once no longer used, or rename to serve_with_no_tls or smth
pub fn serve_routes_with_listener_and_shutdown(
    routes: BoxedFilter<(OldResponse<Body>,)>,
    graceful_shutdown_fut: impl Future<Output = ()> + Send + 'static,
    listener: TcpListener,
    task_name: impl Into<String>,
    span: Span,
) -> anyhow::Result<(LxTask<()>, SocketAddr)> {
    serve_routes_with_listener_and_shutdown_boxed(
        routes,
        Box::pin(graceful_shutdown_fut),
        listener,
        task_name.into(),
        span,
    )
}

// Reduce some code bloat by boxing the warp routes and shutdown future.
fn serve_routes_with_listener_and_shutdown_boxed(
    routes: BoxedFilter<(OldResponse<Body>,)>,
    graceful_shutdown_fut: BoxFuture<'static, ()>,
    listener: TcpListener,
    task_name: String,
    span: Span,
) -> anyhow::Result<(LxTask<()>, SocketAddr)> {
    let api_service = warp::service(routes.with(trace_requests(span.id())));
    let make_service = hyper_old::service::make_service_fn(move |_| {
        let api_service_clone = api_service.clone();
        async move { Ok::<_, Infallible>(api_service_clone) }
    });
    let server = hyper_old::Server::from_tcp(listener)
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
/// const API_SPAN_NAME: &str = "(my-api)";
/// let api_span = info!(parent: None, API_SPAN_NAME);
/// let routes = node_proxy.or(backend_apis).or(app_gateway_api).boxed();
/// let instrumented_routes = routes.with(rest::trace_requests(api_span.id()));
/// ```
fn trace_requests(
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

/// A warp helper that converts `Result<T, E>` into [`OldResponse<Body>`].
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
) -> OldResponse<Body> {
    match reply_res {
        Ok(data) => build_json_response(OldStatusCode::OK, &data),
        Err(err) => build_json_response(err.to_old_http_status(), &err.into()),
    }
}

/// Like [`into_response`], but converts `T` into [`OldResponse<Body>`]. This fn
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
pub fn into_succ_response<T: Serialize>(data: T) -> OldResponse<Body> {
    build_json_response(OldStatusCode::OK, &data)
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
) -> OldResponse<Body> {
    match reply_res {
        Ok(data) =>
            build_json_response_inner(OldStatusCode::OK, Ok(data.into())),
        Err(err) => build_json_response(err.to_old_http_status(), &err.into()),
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
) -> Result<OldResponse<Body>, Rejection> {
    if let Some(err) = err.find::<E>() {
        let status = err.to_old_http_status();
        // TODO(phlip9): find returns &E... figure out how to remove clone
        let err: ErrorResponse = err.clone().into();
        Ok(build_json_response(status, &err))
    } else {
        Err(err)
    }
}

/// Constructs a JSON [`OldResponse<Body>`] from the given data and status code.
/// If serialization fails for some reason (unlikely), log the error,
/// default to an empty body, and override the status code to 500.
fn build_json_response<T: Serialize>(
    status: OldStatusCode,
    data: &T,
) -> OldResponse<Body> {
    build_json_response_inner(status, serde_json::to_vec(data).map(Bytes::from))
}

fn build_json_response_inner(
    mut status: OldStatusCode,
    maybe_json: Result<Bytes, serde_json::Error>,
) -> OldResponse<Body> {
    let body = maybe_json.map(Body::from).unwrap_or_else(|e| {
        error!(target: trace::TARGET, "Couldn't serialize response: {e:#}");
        status = OldStatusCode::INTERNAL_SERVER_ERROR;
        Body::empty()
    });

    OldResponse::builder()
        .header(
            OLD_CONTENT_TYPE,
            OldHeaderValue::from_static("application/json"),
        )
        .status(status)
        .body(body)
        // Only the header could have errored by this point
        .expect("Invalid hard-coded header")
}

/// Converts the concrete, non-generic Rest response result to the specific
/// API's result type.
///
/// On success, this json deserializes the response body. On error, this
/// converts the generic [`ErrorResponse`] or [`CommonApiError`] into the
/// specific API error type, like
/// [`BackendApiError`](crate::api::error::BackendApiError)
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
                let msg = format!("Failed to deser response as json: {err:#}");
                CommonApiError::new(kind, msg)
            })?),
        Ok(Err(err_api)) => Err(E::from(err_api)),
        Err(err_client) => Err(E::from(err_client)),
    }
}

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
        E: ApiError,
    {
        let request = request_builder.build().map_err(CommonApiError::from)?;
        let (request_span, trace_id) =
            trace::client::request_span(&request, self.from, self.to);
        let response = self
            .send_with_retries_inner(request, retries, stop_codes, &trace_id)
            .instrument(request_span)
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
