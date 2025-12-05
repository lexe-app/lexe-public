// This is the only place where we are allowed to use e.g. `Json` and `Query`.
#![allow(clippy::disallowed_types)]

//! This module provides various API server utilities.
//!
//! # Serving
//!
//! Methods to serve a [`Router`] with a fallback handler (for unmatched paths),
//! tracing / request instrumentation, backpressure, load shedding, concurrency
//! limits, server-side timeouts, TLS, and graceful shutdown:
//!
//! - [`build_server_fut`]
//! - [`build_server_fut_with_listener`]
//! - [`spawn_server_task`]
//! - [`spawn_server_task_with_listener`]
//!
//! # Extractors to get data from requests:
//!
//! - [`LxJson`] to deserialize from HTTP body JSON
//! - [`LxQuery`] to deserialize from query strings
//!
//! # [`IntoResponse`] types / impls for building Lexe API-conformant responses:
//!
//! - [`LxJson`] type for returning success responses as JSON
//! - All [`ApiError`]s and [`CommonApiError`] impl [`IntoResponse`]
//! - [`LxRejection`] for notifying clients of bad JSON, query strings, etc.
//!
//! [`ApiError`]: lexe_api_core::error::ApiError
//! [`CommonApiError`]: lexe_api_core::error::CommonApiError
//! [`Router`]: axum::Router
//! [`IntoResponse`]: axum::response::IntoResponse
//! [`LxJson`]: crate::server::LxJson
//! [`LxQuery`]: crate::server::extract::LxQuery
//! [`LxRejection`]: crate::server::LxRejection
//! [`build_server_fut`]: crate::server::build_server_fut
//! [`build_server_fut_with_listener`]: crate::server::build_server_fut_with_listener
//! [`spawn_server_task`]: crate::server::spawn_server_task
//! [`spawn_server_task_with_listener`]: crate::server::spawn_server_task_with_listener

use std::{
    borrow::Cow,
    convert::Infallible,
    fmt::{self, Display},
    future::Future,
    net::{SocketAddr, TcpListener},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use axum::{
    Router, ServiceExt as AxumServiceExt,
    error_handling::HandleErrorLayer,
    extract::{
        DefaultBodyLimit, FromRequest,
        rejection::{
            BytesRejection, JsonRejection, PathRejection, QueryRejection,
        },
    },
    response::IntoResponse,
    routing::RouterIntoService,
};
use axum_server::tls_rustls::RustlsConfig;
use bytes::Bytes;
use common::{
    api::auth::{self, Scope},
    ed25519,
};
use http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use lexe_api_core::{
    axum_helpers,
    error::{CommonApiError, CommonErrorKind},
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use serde::{Serialize, de::DeserializeOwned};
use tower::{
    Layer, buffer::BufferLayer, limit::ConcurrencyLimitLayer,
    load_shed::LoadShedLayer, timeout::TimeoutLayer, util::MapRequestLayer,
};
use tracing::{Instrument, debug, error, info, warn};

use crate::{rest, trace};

/// The grace period passed to [`axum_server::Handle::graceful_shutdown`] during
/// which new connections are refused and we wait for existing connections to
/// terminate before initiating a hard shutdown.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(3);
/// The maximum time we'll wait for a server to complete shutdown.
pub const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
lexe_std::const_assert!(
    SHUTDOWN_GRACE_PERIOD.as_secs() < SERVER_SHUTDOWN_TIMEOUT.as_secs()
);

/// The default maximum time a server can spend handling a request.
pub const SERVER_HANDLER_TIMEOUT: Duration = Duration::from_secs(25);
lexe_std::const_assert!(
    rest::API_REQUEST_TIMEOUT.as_secs() > SERVER_HANDLER_TIMEOUT.as_secs()
);

/// A configuration object for Axum / Tower middleware.
///
/// Defaults:
///
/// ```
/// # use std::time::Duration;
/// # use lexe_api::server::LayerConfig;
/// assert_eq!(
///     LayerConfig::default(),
///     LayerConfig {
///         body_limit: 16384,
///         buffer_size: 4096,
///         concurrency: 4096,
///         handling_timeout: Duration::from_secs(25),
///         default_fallback: true,
///     }
/// );
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerConfig {
    /// The maximum size of the request body in bytes.
    /// Helps prevent DoS, but may need to be increased for some services.
    pub body_limit: usize,
    /// The size of the work buffer for our service.
    /// Allows the server to immediately work on more queued requests when a
    /// request completes, and prevents a large backlog from building up.
    pub buffer_size: usize,
    /// The maximum # of requests we'll process at once.
    /// Helps prevent the CPU from maxing out, resulting in thrashing.
    pub concurrency: usize,
    /// The maximum time a server can spend handling a request. Helps prevent
    /// degenerate cases which take abnormally long to process from crowding
    /// out normal workloads.
    pub handling_timeout: Duration,
    /// Whether to add Lexe's default [`Router::fallback`] to the [`Router`].
    /// The [`Router::fallback`] is called if no routes were matched;
    /// Lexe's [`default_fallback`] returns a "bad endpoint" rejection along
    /// with the requested method and path.
    ///
    /// If you need to set a custom fallback, set this to [`false`], otherwise
    /// your custom fallback will be clobbered by Lexe's [`default_fallback`].
    /// NOTE, however, that the caller is responsible for ensuring that the
    /// [`Router`] has a fallback configured in this case.
    pub default_fallback: bool,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            // 16KiB is sufficient for most Lexe services.
            body_limit: 16384,
            // TODO(max): We are using very high values right now because it
            // doesn't make sense to constrain anything until we have run some
            // load tests to profile performance and see what breaks.
            buffer_size: 4096,
            concurrency: 4096,
            handling_timeout: SERVER_HANDLER_TIMEOUT,
            default_fallback: true,
        }
    }
}

// --- Server helpers --- //

/// Construct a server URL given the [`TcpListener::local_addr`] from by a
/// server's [`TcpListener`], and its DNS name.
///
/// ex: `https://lexe.app` (port=443)
/// ex: `https://relay.lexe.app:4396`
/// ex: `http://[::1]:8080`
//
// We have a fn to build the url because it's easy to mess up.
pub fn build_server_url(
    // The output of `TcpListener::local_addr`
    listener_addr: SocketAddr,
    // Primary DNS name
    maybe_dns: Option<&str>,
) -> String {
    match maybe_dns {
        Some(dns_name) => {
            let port = listener_addr.port();
            if port == 443 {
                format!("https://{dns_name}")
            } else {
                format!("https://{dns_name}:{port}")
            }
        }
        None => format!("http://{listener_addr}"),
    }
}

/// Constructs an API server future which can be spawned into a task.
/// Additionally returns the server url.
///
/// Use this helper when it is useful to poll multiple futures in a single task
/// to reduce the amount of task nesting / indirection. If there is only one
/// future that needs to be driven, use [`spawn_server_task`] instead.
///
/// Errors if the [`TcpListener`] failed to bind or return its local address.
/// Returns the server future along with the bound socket address.
// Avoids generic parameters to prevent binary bloat.
// Returns unnamed `impl Future` to avoid Pin<Box<T>> deref cost.
pub fn build_server_fut(
    bind_addr: SocketAddr,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + primary DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: &str,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: NotifyOnce,
) -> anyhow::Result<(impl Future<Output = ()>, String)> {
    let listener =
        TcpListener::bind(bind_addr).context("Could not bind TCP listener")?;
    let (server_fut, primary_server_url) = build_server_fut_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_and_dns,
        server_span_name,
        server_span,
        shutdown,
    )
    .context("Could not build server future")?;
    Ok((server_fut, primary_server_url))
}

/// [`build_server_fut`] but takes a [`TcpListener`] instead of [`SocketAddr`].
// Avoids generic parameters to prevent binary bloat.
// Returns unnamed `impl Future` to avoid Pin<Box<T>> deref cost.
pub fn build_server_fut_with_listener(
    listener: TcpListener,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + primary DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: &str,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    mut shutdown: NotifyOnce,
) -> anyhow::Result<(impl Future<Output = ()> + use<>, String)> {
    let (maybe_tls_config, maybe_dns) = maybe_tls_and_dns.unzip();
    let listener_addr = listener
        .local_addr()
        .context("Could not get listener local address")?;
    let primary_server_url = build_server_url(listener_addr, maybe_dns);
    info!("Url for {server_span_name}: {primary_server_url}");

    // Add Lexe's default fallback if it is enabled in the LayerConfig.
    let router = if layer_config.default_fallback {
        router.fallback(default_fallback)
    } else {
        router
    };

    // Used to annotate the service / request / response types
    // at each point in the ServiceBuilder chains.
    type HyperService = RouterIntoService<hyper::body::Incoming, ()>;
    type AxumService = RouterIntoService<axum::body::Body, ()>;
    type HyperReq = http::Request<hyper::body::Incoming>;
    type AxumReq = http::Request<axum::body::Body>;
    type AxumResp = http::Response<axum::body::Body>;
    type TraceResp = http::Response<
        tower_http::trace::ResponseBody<
            axum::body::Body,
            tower_http::classify::NeverClassifyEos<anyhow::Error>,
            (),
            trace::server::LxOnEos,
            trace::server::LxOnFailure,
        >,
    >;

    // The outer middleware stack which wraps the entire Router.
    //
    // Axum docs explain ordering better than tower's ServiceBuilder docs do:
    // https://docs.rs/axum/latest/axum/middleware/index.html#ordering
    // Basically, requests go from top to bottom and responses bottom to top.
    let outer_middleware = tower::ServiceBuilder::new()
        .check_service::<HyperService, HyperReq, AxumResp, Infallible>()
        // Log everything on its way in and out, even load-shedded requests.
        // This layer changes the response type.
        .layer(trace::server::trace_layer(server_span.clone()))
        .check_service::<HyperService, HyperReq, TraceResp, Infallible>()
        // Run our post-processor which can modify responses *after* the Axum
        // Router has constructed the response.
        .layer(tower::util::MapResponseLayer::new(
            middleware::post_process_response,
        ))
        .check_service::<HyperService, HyperReq, TraceResp, Infallible>();

    // The inner middleware stack which is cloned to each route in the Router.
    // We put most of the layers here because it is a lot easier to work with
    // axum types; moving these outside quickly degenerates into type hell.
    let inner_middleware = tower::ServiceBuilder::new()
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Immediately reject anything with a CONTENT_LENGTH over the limit.
        .layer(axum::middleware::map_request_with_state(
            layer_config.body_limit,
            middleware::check_content_length_header,
        ))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Set the default request body limit for all requests. This adds a
        // `DefaultBodyLimitKind` (private axum type) into the request
        // extensions so that any inner layers or extractors which call
        // `axum::RequestExt::[with|into]_limited_body` will pick it up.
        // NOTE that many of our extractors transitively rely on the Bytes
        // extractor which will default to a 2MB limit if this is not set.
        .layer(DefaultBodyLimit::max(layer_config.body_limit))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Here, we explicitly apply the body limit from the request extensions,
        // transforming the request body type into `http_body_util::Limited`.
        .layer(MapRequestLayer::new(axum::RequestExt::with_limited_body))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Handles errors from the load_shed, buffer, and concurrency layers.
        .layer(HandleErrorLayer::new(|_: tower::BoxError| async move {
            CommonApiError {
                kind: CommonErrorKind::AtCapacity,
                msg: "Service is at capacity; retry later".to_owned(),
            }
        }))
        // Returns an `Err` if the inner service returns `Poll::Pending`.
        // Helps prevent OOM when combined with the buffer or concurrency layer.
        .layer(LoadShedLayer::new())
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Returns Poll::Pending when the buffer is full (backpressure).
        // Allows the server to immediately work on more queued requests when a
        // request completes, and prevents a large backlog from building up.
        // Note that while the layer is often cloned, the buffer itself is not.
        .layer(BufferLayer::new(layer_config.buffer_size))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Returns `Poll::Pending` when the concurrency limit has been reached.
        // Helps prevent the CPU from maxing out, resulting in thrashing.
        .layer(ConcurrencyLimitLayer::new(layer_config.concurrency))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>()
        // Handles errors generated by the timeout layer.
        .layer(HandleErrorLayer::new(|_: tower::BoxError| async move {
            CommonApiError {
                kind: CommonErrorKind::Server,
                msg: "Server timed out handling request".to_owned(),
            }
        }))
        // Returns an error if the inner service takes longer than the timeout
        // to handle the request. Prevents degenerate cases which take
        // abnormally long to process from crowding out normal workloads.
        .layer(TimeoutLayer::new(layer_config.handling_timeout))
        .check_service::<AxumService, AxumReq, AxumResp, Infallible>();

    // Apply inner middleware
    let layered_router = router.layer(inner_middleware);
    // Convert into Service
    let router_service = layered_router.into_service::<hyper::body::Incoming>();
    // Apply outer middleware
    let layered_service = Layer::layer(&outer_middleware, router_service);
    // Convert into MakeService
    let make_service = layered_service.into_make_service();

    let handle = axum_server::Handle::new();
    let handle_clone = handle.clone();
    let server_fut = async {
        let serve_result = match maybe_tls_config {
            Some(tls_config) => {
                let axum_tls_config = RustlsConfig::from_config(tls_config);
                axum_server::from_tcp_rustls(listener, axum_tls_config)
                    .handle(handle_clone)
                    .serve(make_service)
                    .await
            }
            None =>
                axum_server::from_tcp(listener)
                    .handle(handle_clone)
                    .serve(make_service)
                    .await,
        };

        serve_result
            // See axum_server::Server::serve docs for why this can't error
            .expect("No binding + axum MakeService::poll_ready never errors");
    };

    let graceful_shutdown_fut = async move {
        shutdown.recv().await;
        info!("Shutting down API server");
        // The 'grace period' is a period of time during which new connections
        // are refused and `axum_server::Server::serve` waits for all current
        // connections to terminate. If `None`, the server waits indefinitely
        // for current connections to terminate; if `Some`, the server will
        // initiate a hard shutdown after the grace period has elapsed. We use
        // Some(_) with a relatively short grace period because (1) our handlers
        // shouldn't take long to return and (2) we sometimes see connections
        // failing to terminate for servers which have a /shutdown endpoint.
        handle.graceful_shutdown(Some(SHUTDOWN_GRACE_PERIOD));
    };

    let combined_fut = async {
        tokio::pin!(server_fut);
        tokio::select! {
            biased; // Ensure graceful shutdown future finishes first
            () = graceful_shutdown_fut => (),
            _ = &mut server_fut => return error!("Server exited early"),
        }
        match tokio::time::timeout(SERVER_SHUTDOWN_TIMEOUT, server_fut).await {
            Ok(()) => info!("API server finished"),
            Err(_) => warn!("API server timed out during shutdown"),
        }
    }
    .instrument(server_span);

    Ok((combined_fut, primary_server_url))
}

/// [`build_server_fut`] but additionally spawns the server future into an
/// instrumented server task and logs the full URL used to access the server.
/// Returns the server task and server url.
pub fn spawn_server_task(
    bind_addr: SocketAddr,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + primary DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: Cow<'static, str>,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: NotifyOnce,
) -> anyhow::Result<(LxTask<()>, String)> {
    let listener = TcpListener::bind(bind_addr)
        .context(bind_addr)
        .context("Failed to bind TcpListener")?;

    let (server_task, primary_server_url) = spawn_server_task_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_and_dns,
        server_span_name,
        server_span,
        shutdown,
    )
    .context("spawn_server_task_with_listener failed")?;

    Ok((server_task, primary_server_url))
}

/// [`spawn_server_task`] but takes [`TcpListener`] instead of [`SocketAddr`].
pub fn spawn_server_task_with_listener(
    listener: TcpListener,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + primary DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: Cow<'static, str>,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: NotifyOnce,
) -> anyhow::Result<(LxTask<()>, String)> {
    let (server_fut, primary_server_url) = build_server_fut_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_and_dns,
        &server_span_name,
        server_span.clone(),
        shutdown,
    )
    .context("Failed to build server future")?;

    let server_task =
        LxTask::spawn_with_span(server_span_name, server_span, server_fut);

    Ok((server_task, primary_server_url))
}

// --- LxJson --- //

/// A version of [`axum::Json`] which conforms to Lexe's (JSON) API.
/// It can be used as either an extractor or a response.
///
/// - As an extractor: rejections return [`LxRejection`].
/// - As a success response:
///   - Serialization success returns an [`http::Response`] with JSON body.
///   - Serialization failure returns a [`ErrorResponse`].
///
/// [`axum::Json`] is banned because:
///
/// - Rejections return [`JsonRejection`] which is just a string HTTP body.
/// - Response serialization failures likewise return just a string body.
///
/// NOTE: This must only be used for forming *success* API responses,
/// i.e. `T` in `Result<T, E>`, because its [`IntoResponse`] impl uses
/// [`StatusCode::OK`]. Our API error types, while also serialized as JSON,
/// have separate [`IntoResponse`] impls which return error statuses.
///
/// [`ErrorResponse`]: lexe_api_core::error::ErrorResponse
pub struct LxJson<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for LxJson<T> {
    type Rejection = LxRejection;

    async fn from_request(
        req: http::Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // `axum::Json`'s from_request impl is fine but its rejection is not
        axum::Json::from_request(req, state)
            .await
            .map(|axum::Json(t)| Self(t))
            .map_err(LxRejection::from)
    }
}

impl<T: Serialize> IntoResponse for LxJson<T> {
    fn into_response(self) -> http::Response<axum::body::Body> {
        axum_helpers::build_json_response(StatusCode::OK, &self.0)
    }
}

impl<T: Clone> Clone for LxJson<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy> Copy for LxJson<T> {}

impl<T: fmt::Debug> fmt::Debug for LxJson<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        T::fmt(&self.0, f)
    }
}

impl<T: Eq + PartialEq> Eq for LxJson<T> {}

impl<T: PartialEq> PartialEq for LxJson<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

// --- LxBytes --- //

/// A version of [`Bytes`] which conforms to Lexe's (binary) API.
/// - [`axum`] has implementations of [`FromRequest`] and [`IntoResponse`] for
///   [`Bytes`], but these implementations are not Lexe API-conformant.
/// - This type can be used as either an extractor or a success response, and
///   should always be used instead of [`Bytes`] in these server contexts.
/// - It is still fine to use [`Bytes`] on the client side.
///
/// - As an extractor: rejections return [`LxRejection`].
/// - As a success response:
///   - Returns an [`http::Response`] with a binary body.
///
///   - Any failure encountered in extraction or creation should produce an
///     [`ErrorResponse`].
///
/// The regular impls are non-conformant because:
///
/// - Rejections return [`BytesRejection`] which is just a string HTTP body.
///
/// NOTE: This must only be used for forming *success* API responses,
/// i.e. `LxBytes` in `Result<LxBytes, E>`, because its [`IntoResponse`] impl
/// uses [`StatusCode::OK`]. Our API error types are serialized as JSON and
/// have separate [`IntoResponse`] impls which return error statuses.
///
/// [`ErrorResponse`]: lexe_api_core::error::ErrorResponse
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct LxBytes(pub Bytes);

impl<S: Send + Sync> FromRequest<S> for LxBytes {
    type Rejection = LxRejection;

    async fn from_request(
        req: http::Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // `Bytes`'s from_request impl is fine but its rejection is not
        Bytes::from_request(req, state)
            .await
            .map(Self)
            .map_err(LxRejection::from)
    }
}

/// The [`Bytes`] [`IntoResponse`] impl is almost exactly the same,
/// except it returns the wrong HTTP version.
impl IntoResponse for LxBytes {
    fn into_response(self) -> http::Response<axum::body::Body> {
        let http_body = http_body_util::Full::new(self.0);
        let axum_body = axum::body::Body::new(http_body);

        axum_helpers::default_response_builder()
            .header(
                CONTENT_TYPE,
                // Or `HeaderValue::from_static(mime::APPLICATION_OCTET_STREAM)`
                HeaderValue::from_static("application/octet-stream"),
            )
            .status(StatusCode::OK)
            .body(axum_body)
            .expect("All operations here should be infallible")
    }
}

impl<T: Into<Bytes>> From<T> for LxBytes {
    fn from(t: T) -> Self {
        Self(t.into())
    }
}

// --- LxRejection --- //

/// Our own [`axum::extract::rejection`] type with an [`IntoResponse`] impl
/// which conforms to Lexe's API. Contains the source rejection's error text.
pub struct LxRejection {
    /// Which [`axum::extract::rejection`] this [`LxRejection`] was built from.
    kind: LxRejectionKind,
    /// The error text of the source rejection, or additional context.
    source_msg: String,
}

/// The source of this [`LxRejection`].
enum LxRejectionKind {
    // -- From `axum::extract::rejection` -- //
    /// [`BytesRejection`]
    Bytes,
    /// [`JsonRejection`]
    Json,
    /// [`PathRejection`]
    Path,
    /// [`QueryRejection`]
    Query,

    // -- Other -- //
    /// Bearer authentication failed
    Unauthenticated,
    /// Client is not authorized to access this resource
    Unauthorized,
    /// Client request did not match any paths in the [`Router`].
    BadEndpoint,
    /// Request body length over limit
    BodyLengthOverLimit,
    /// [`ed25519::Error`]
    Ed25519,
    /// Gateway proxy
    Proxy,
}

// Use explicit `.map_err()`s instead of From impls for non-obvious conversions
impl LxRejection {
    pub fn from_ed25519(error: ed25519::Error) -> Self {
        Self {
            kind: LxRejectionKind::Ed25519,
            source_msg: format!("{error:#}"),
        }
    }

    pub fn from_bearer_auth(error: auth::Error) -> Self {
        Self {
            kind: LxRejectionKind::Unauthenticated,
            source_msg: format!("{error:#}"),
        }
    }

    pub fn scope_unauthorized(
        granted_scope: &Scope,
        requested_scope: &Scope,
    ) -> Self {
        Self {
            kind: LxRejectionKind::Unauthorized,
            source_msg: format!(
                "granted scope: {granted_scope:?}, requested scope: {requested_scope:?}"
            ),
        }
    }

    pub fn proxy(error: impl Display) -> Self {
        Self {
            kind: LxRejectionKind::Proxy,
            source_msg: format!("{error:#}"),
        }
    }
}

impl From<BytesRejection> for LxRejection {
    fn from(bytes_rejection: BytesRejection) -> Self {
        Self {
            kind: LxRejectionKind::Bytes,
            source_msg: bytes_rejection.body_text(),
        }
    }
}

impl From<JsonRejection> for LxRejection {
    fn from(json_rejection: JsonRejection) -> Self {
        Self {
            kind: LxRejectionKind::Json,
            source_msg: json_rejection.body_text(),
        }
    }
}

impl From<PathRejection> for LxRejection {
    fn from(path_rejection: PathRejection) -> Self {
        Self {
            kind: LxRejectionKind::Path,
            source_msg: path_rejection.body_text(),
        }
    }
}

impl From<QueryRejection> for LxRejection {
    fn from(query_rejection: QueryRejection) -> Self {
        Self {
            kind: LxRejectionKind::Query,
            source_msg: query_rejection.body_text(),
        }
    }
}

impl IntoResponse for LxRejection {
    fn into_response(self) -> http::Response<axum::body::Body> {
        // TODO(phlip9): authn+authz+badendpoint rejections should return
        // standard status codes, not just 400.
        let kind = CommonErrorKind::Rejection;
        // "Bad JSON: Failed to deserialize the JSON body into the target type"
        let kind_msg = self.kind.to_msg();
        let source_msg = &self.source_msg;
        let msg = format!("Rejection: {kind_msg}: {source_msg}");
        // Log the rejection now since our trace layer can't access this info
        warn!("{msg}");
        let common_error = CommonApiError { kind, msg };
        common_error.into_response()
    }
}

impl LxRejectionKind {
    /// A generic error message for this rejection kind.
    fn to_msg(&self) -> &'static str {
        match self {
            Self::Bytes => "Bad request bytes",
            Self::Json => "Client provided bad JSON",
            Self::Path => "Client provided bad path parameter",
            Self::Query => "Client provided bad query string",

            Self::Unauthenticated => "Invalid bearer auth",
            Self::Unauthorized => "Not authorized to access this resource",
            Self::BadEndpoint => "Client requested a non-existent endpoint",
            Self::BodyLengthOverLimit => "Request body length over limit",
            Self::Ed25519 => "Ed25519 error",
            Self::Proxy => "Proxy error",
        }
    }
}

// --- Extractors --- //

pub mod extract {
    use axum::extract::FromRequestParts;

    use super::*;

    /// Lexe API-compliant version of [`axum::extract::Query`].
    pub struct LxQuery<T>(pub T);

    impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for LxQuery<T> {
        type Rejection = LxRejection;

        async fn from_request_parts(
            parts: &mut http::request::Parts,
            state: &S,
        ) -> Result<Self, Self::Rejection> {
            axum::extract::Query::from_request_parts(parts, state)
                .await
                .map(|axum::extract::Query(t)| Self(t))
                .map_err(LxRejection::from)
        }
    }

    impl<T: Clone> Clone for LxQuery<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T: fmt::Debug> fmt::Debug for LxQuery<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            T::fmt(&self.0, f)
        }
    }

    impl<T: Eq + PartialEq> Eq for LxQuery<T> {}

    impl<T: PartialEq> PartialEq for LxQuery<T> {
        fn eq(&self, other: &Self) -> bool {
            self.0.eq(&other.0)
        }
    }

    /// Lexe API-compliant version of [`axum::extract::Path`].
    pub struct LxPath<T>(pub T);

    impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S>
        for LxPath<T>
    {
        type Rejection = LxRejection;

        async fn from_request_parts(
            parts: &mut http::request::Parts,
            state: &S,
        ) -> Result<Self, Self::Rejection> {
            axum::extract::Path::from_request_parts(parts, state)
                .await
                .map(|axum::extract::Path(t)| Self(t))
                .map_err(LxRejection::from)
        }
    }

    impl<T: Clone> Clone for LxPath<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T: fmt::Debug> fmt::Debug for LxPath<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            T::fmt(&self.0, f)
        }
    }

    impl<T: Eq + PartialEq> Eq for LxPath<T> {}

    impl<T: PartialEq> PartialEq for LxPath<T> {
        fn eq(&self, other: &Self) -> bool {
            self.0.eq(&other.0)
        }
    }
}

// --- Custom middleware --- //

pub mod middleware {
    use axum::extract::State;
    use http::HeaderName;

    use super::*;

    /// The header name used for response post-processing signals.
    pub static POST_PROCESS_HEADER: HeaderName =
        HeaderName::from_static("lx-post-process");

    /// Checks the `CONTENT_LENGTH` header and returns an early rejection if the
    /// contained value exceeds our configured body limit. This optimization
    /// allows us to avoid unnecessary work processing the request further.
    ///
    /// NOTE: This does not enforce the body length!! Use [`DefaultBodyLimit`]
    /// in combination with [`axum::RequestExt::with_limited_body`] to do so.
    pub async fn check_content_length_header<B>(
        // `LayerConfig::body_limit`
        State(config_body_limit): State<usize>,
        request: http::Request<B>,
    ) -> Result<http::Request<B>, LxRejection> {
        let maybe_content_length = request
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value_str| usize::from_str(value_str).ok());

        // If a limit is configured and the header value exceeds it, reject.
        if let Some(content_length) = maybe_content_length
            && content_length > config_body_limit
        {
            return Err(LxRejection {
                kind: LxRejectionKind::BodyLengthOverLimit,
                source_msg: "Content length header over limit".to_owned(),
            });
        }

        Ok(request)
    }

    /// A post-processor which can be used to modify the [`http::Response`]s
    /// returned by an [`axum::Router`]. This is done by signalling the desired
    /// modification in a fake [`POST_PROCESS_HEADER`] which is also removed
    /// during post-processing. This can be used to override Axum defaults
    /// which one does not have access to from within the [`Router`]. Currently,
    /// this only supports a "remove-content-length" command which removes the
    /// content-length header set by Axum, but can be easily extended.
    pub(super) fn post_process_response(
        mut response: http::Response<axum::body::Body>,
    ) -> http::Response<axum::body::Body> {
        let value = match response.headers_mut().remove(&POST_PROCESS_HEADER) {
            Some(v) => v,
            None => return response,
        };

        match value.as_bytes() {
            b"remove-content-length" => {
                response.headers_mut().remove(http::header::CONTENT_LENGTH);
                debug!("Post process: Removed content-length header");
            }
            unknown => {
                let unknown_str = String::from_utf8_lossy(unknown);
                warn!("Post process: Invalid header value: {unknown_str}");
            }
        }

        response
    }
}

// --- Helpers --- //

/// Lexe's default fallback [`Handler`](axum::handler::Handler).
/// Returns a "bad endpoint" rejection along with the requested method and path.
pub async fn default_fallback(
    method: http::Method,
    uri: http::Uri,
) -> LxRejection {
    let path = uri.path();
    LxRejection {
        kind: LxRejectionKind::BadEndpoint,
        // e.g. "POST /app/node_info"
        source_msg: format!("{method} {path}"),
    }
}
