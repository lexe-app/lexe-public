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
//! [`ApiError`]: crate::api::error::ApiError
//! [`CommonApiError`]: crate::api::error::CommonApiError
//! [`Router`]: axum::Router
//! [`IntoResponse`]: axum::response::IntoResponse
//! [`LxJson`]: crate::api::server::LxJson
//! [`LxQuery`]: crate::api::server::extract::LxQuery
//! [`LxRejection`]: crate::api::server::LxRejection
//! [`build_server_fut`]: crate::api::server::build_server_fut
//! [`build_server_fut_with_listener`]: crate::api::server::build_server_fut_with_listener
//! [`spawn_server_task`]: crate::api::server::spawn_server_task
//! [`spawn_server_task_with_listener`]: crate::api::server::spawn_server_task_with_listener

use std::{
    fmt,
    future::Future,
    io,
    net::{SocketAddr, TcpListener},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use axum::{
    error_handling::HandleErrorLayer,
    extract::{
        rejection::{BytesRejection, JsonRejection, QueryRejection},
        DefaultBodyLimit, FromRequest,
    },
    response::IntoResponse,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use http::{header::CONTENT_TYPE, HeaderValue, StatusCode, Version};
use serde::{de::DeserializeOwned, Serialize};
use tower::{
    buffer::BufferLayer, limit::ConcurrencyLimitLayer,
    load_shed::LoadShedLayer, timeout::TimeoutLayer, util::MapRequestLayer,
};
use tracing::{debug, error, info, warn, Instrument};

use super::auth;
use crate::{
    api::error::{
        CommonApiError, CommonErrorKind, ErrorResponse, ToHttpStatus,
    },
    const_assert, ed25519,
    shutdown::ShutdownChannel,
    task::LxTask,
};

/// The HTTP version returned in our server responses.
const HTTP_VERSION: Version = Version::HTTP_2;
/// The grace period passed to [`axum_server::Handle::graceful_shutdown`] during
/// which new connections are refused and we wait for existing connections to
/// terminate before initiating a hard shutdown.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(3);
/// The maximum time we'll wait for a server to complete shutdown.
const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const_assert!(
    SHUTDOWN_GRACE_PERIOD.as_secs() < SERVER_SHUTDOWN_TIMEOUT.as_secs()
);

/// A configuration object for Axum / Tower middleware.
///
/// Defaults:
///
/// ```
/// # use std::time::Duration;
/// # use common::api::server::LayerConfig;
/// assert_eq!(
///     LayerConfig::default(),
///     LayerConfig {
///         body_limit: Some(16384),
///         load_shed: true,
///         buffer_size: Some(4096),
///         concurrency: Some(4096),
///         handling_timeout: Some(Duration::from_secs(15)),
///     }
/// );
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerConfig {
    /// The maximum size of the request body in bytes ([`None`] to disable).
    /// Helps prevent DoS, but may need to be increased for some services.
    pub body_limit: Option<usize>,
    /// Whether to shed load when the service has reached capacity.
    /// Helps prevent OOM when combined with the buffer or concurrency layer.
    pub load_shed: bool,
    /// The size of the work buffer for our service ([`None`] to disable).
    /// Allows the server to immediately work on more queued requests when a
    /// request completes, and prevents a large backlog from building up.
    pub buffer_size: Option<usize>,
    /// The maximum # of requests we'll process at once ([`None`] to disable).
    /// Helps prevent the CPU from maxing out, resulting in thrashing.
    pub concurrency: Option<usize>,
    /// The maximum time a server can spend handling a request.
    /// ([`None`] to disable). Helps prevent degenerate cases which take
    /// abnormally long to process from crowding out normal workloads.
    pub handling_timeout: Option<Duration>,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            // 16KiB is sufficient for most Lexe services.
            body_limit: Some(16384),
            load_shed: true,
            // TODO(max): We are using very high values right now because it
            // doesn't make sense to constrain anything until we have run some
            // load tests to profile performance and see what breaks.
            buffer_size: Some(4096),
            concurrency: Some(4096),
            handling_timeout: Some(Duration::from_secs(15)),
        }
    }
}

// --- Server helpers --- //

/// Constructs an API server future which can be spawned into a task.
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
    maybe_tls_config: Option<Arc<rustls::ServerConfig>>,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: ShutdownChannel,
) -> Result<(impl Future<Output = ()>, SocketAddr), io::Error> {
    let listener = TcpListener::bind(bind_addr)?;
    let local_addr = listener.local_addr()?;
    let server_fut = build_server_fut_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_config,
        server_span,
        shutdown,
    );
    Ok((server_fut, local_addr))
}

/// [`build_server_fut`] but takes a [`TcpListener`] instead of [`SocketAddr`].
// Avoids generic parameters to prevent binary bloat.
// Returns unnamed `impl Future` to avoid Pin<Box<T>> deref cost.
pub fn build_server_fut_with_listener(
    listener: TcpListener,
    router: Router<()>,
    layer_config: LayerConfig,
    maybe_tls_config: Option<Arc<rustls::ServerConfig>>,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    mut shutdown: ShutdownChannel,
) -> impl Future<Output = ()> {
    // Add a fallback which triggers if no routes were matched. Returns a
    // "bad endpoint" rejection with the requested path and method.
    let router_with_fallback =
        router.fallback(|method: http::Method, uri: http::Uri| async move {
            let path = uri.path();
            LxRejection {
                kind: LxRejectionKind::BadEndpoint,
                // e.g. "POST /app/node_info"
                source_msg: format!("{method} {path}"),
            }
        });

    // Set up our middleware stack which wraps our Router.
    // Axum docs explain ordering better than tower's ServiceBuilder docs do:
    // https://docs.rs/axum/latest/axum/middleware/index.html#ordering
    // Basically, requests go from top to bottom and responses bottom to top.
    let middleware_stack = tower::ServiceBuilder::new()
        // Log everything on its way in and out, even load-shedded requests.
        .layer(super::trace::server::trace_layer(server_span.clone()))
        // Immediately reject anything with a CONTENT_LENGTH over the limit.
        .layer(axum::middleware::map_request_with_state(
            layer_config.body_limit,
            middleware::check_content_length_header,
        ))
        // Set the default request body limit for all requests. This adds a
        // `DefaultBodyLimitKind` (private axum type) into the request
        // extensions so that any inner layers or extractors which call
        // `axum::RequestExt::[with|into]_limited_body` will pick it up.
        // NOTE that many of our extractors transitively rely on the Bytes
        // extractor which will default to a 2MB limit if this is not set.
        .layer(
            layer_config
                .body_limit
                .map(DefaultBodyLimit::max)
                .unwrap_or_else(DefaultBodyLimit::disable),
        )
        // Here, we explicitly apply the body limit from the request extensions,
        // transforming the request body type into `http_body_util::Limited`.
        .layer(MapRequestLayer::new(axum::RequestExt::with_limited_body))
        // Handles errors from the load_shed, buffer, and concurrency layers.
        .layer(HandleErrorLayer::new(|error| async move {
            CommonApiError {
                kind: CommonErrorKind::AtCapacity,
                msg: format!("Service is at capacity; retry later: {error:#}"),
            }
        }))
        // Returns an error if the inner service returns Poll::Pending.
        // Helps prevent OOM when combined with the buffer or concurrency layer.
        .option_layer(layer_config.load_shed.then(LoadShedLayer::new))
        // Returns Poll::Pending when the buffer is full (backpressure).
        // Allows the server to immediately work on more queued requests when a
        // request completes, and prevents a large backlog from building up.
        .option_layer(layer_config.buffer_size.map(BufferLayer::new))
        // Returns Poll::Pending when the concurrency limit has been reached.
        // Helps prevent the CPU from maxing out, resulting in thrashing.
        .option_layer(layer_config.concurrency.map(ConcurrencyLimitLayer::new))
        // Handles errors generated by the timeout layer.
        .layer(HandleErrorLayer::new(|error| async move {
            CommonApiError {
                kind: CommonErrorKind::Server,
                msg: format!("Server timed out handling request: {error:#}"),
            }
        }))
        // Returns an error if the inner service takes longer than the timeout
        // to handle the request. Prevents degenerate cases which take
        // abnormally long to process from crowding out normal workloads.
        .option_layer(layer_config.handling_timeout.map(TimeoutLayer::new));

    let layered_router = router_with_fallback.layer(middleware_stack);

    let handle = axum_server::Handle::new();
    let handle_clone = handle.clone();
    let server_fut = async {
        let make_service = layered_router.into_make_service();
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

        info!("API server finished");
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

    async {
        tokio::pin!(server_fut);
        tokio::select! {
            biased; // Ensure graceful shutdown future finishes first
            () = graceful_shutdown_fut => (),
            _ = &mut server_fut => return error!("Server exited early"),
        }
        match tokio::time::timeout(SERVER_SHUTDOWN_TIMEOUT, server_fut).await {
            Ok(()) => debug!("API server graceful shutdown success"),
            Err(_) => warn!("API server timed out during shutdown"),
        }
    }
    .instrument(server_span)
}

/// [`build_server_fut`] but additionally spawns the server future into an
/// instrumented server task and logs the full URL used to access the server.
/// Returns the server task and server url.
pub fn spawn_server_task(
    bind_addr: SocketAddr,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: &str,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: ShutdownChannel,
) -> anyhow::Result<(LxTask<()>, String)> {
    let listener = TcpListener::bind(bind_addr)
        .context(bind_addr)
        .context("Failed to bind TcpListener")?;

    let (server_task, server_url) = spawn_server_task_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_and_dns,
        server_span_name,
        server_span,
        shutdown,
    )
    .context("spawn_server_task_with_listener failed")?;

    Ok((server_task, server_url))
}

/// [`spawn_server_task`] but takes [`TcpListener`] instead of [`SocketAddr`].
pub fn spawn_server_task_with_listener(
    listener: TcpListener,
    router: Router<()>,
    layer_config: LayerConfig,
    // TLS config + DNS name
    maybe_tls_and_dns: Option<(Arc<rustls::ServerConfig>, &str)>,
    server_span_name: &str,
    server_span: tracing::Span,
    // Send on this channel to begin a graceful shutdown of the server.
    shutdown: ShutdownChannel,
) -> anyhow::Result<(LxTask<()>, String)> {
    let using_tls = maybe_tls_and_dns.is_some();
    let (maybe_tls_config, maybe_dns_name) = maybe_tls_and_dns.unzip();
    let server_addr = listener
        .local_addr()
        .context("Could not get local address of TcpListener")?;
    let server_fut = build_server_fut_with_listener(
        listener,
        router,
        layer_config,
        maybe_tls_config,
        server_span.clone(),
        shutdown,
    );

    let server_task = LxTask::spawn_named_with_span(
        server_span_name,
        server_span,
        server_fut,
    );

    // Log the url here bc it's easy to mess up: `http[s]://{dns:port,addr}`
    let server_url = if using_tls {
        let dns_name = maybe_dns_name.expect("Must be Some bc using_tls=true");
        let server_port = server_addr.port();
        format!("https://{dns_name}:{server_port}")
    } else {
        format!("http://{server_addr}")
    };
    info!("Url for {server_span_name}: {server_url}");

    Ok((server_task, server_url))
}

// --- LxJson --- //

/// A version of [`axum::Json`] which conforms to Lexe's (JSON) API.
/// It can be used as either an extractor or a response.
///
/// - As an extractor: rejections return [`LxRejection`].
/// - As a response:
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
pub struct LxJson<T>(pub T);

#[async_trait]
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
        build_json_response(StatusCode::OK, &self.0)
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
    /// [`QueryRejection`]
    Query,

    // -- Other -- //
    Auth,
    /// Client request did not match any paths in the [`Router`].
    BadEndpoint,
    /// Request body length over limit
    BodyLengthOverLimit,
    /// [`ed25519::Error`]
    Ed25519,
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
            kind: LxRejectionKind::Auth,
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
        let kind = CommonErrorKind::Rejection;
        // "Bad JSON: Failed to deserialize the JSON body into the target type"
        let kind_msg = self.kind.to_msg();
        let source_msg = &self.source_msg;
        let msg = format!("Rejection: {kind_msg}: {source_msg}");
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
            Self::Query => "Client provided bad query string",

            Self::Auth => "Bad bearer auth token",
            Self::BadEndpoint => "Client requested a non-existent endpoint",
            Self::BodyLengthOverLimit => "Request body length over limit",
            Self::Ed25519 => "Ed25519 error",
        }
    }
}

// --- Extractors --- //

pub mod extract {
    use axum::extract::FromRequestParts;

    use super::*;

    /// Lexe API-compliant version of [`axum::extract::Query`].
    pub struct LxQuery<T>(pub T);

    #[async_trait]
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

    // TODO(max): There will soon be more extractor types here
}

// --- Custom middleware --- //

pub mod middleware {
    use axum::extract::State;

    use super::*;

    /// Checks the `CONTENT_LENGTH` header and returns an early rejection if the
    /// contained value exceeds our configured body limit. This optimization
    /// allows us to avoid unnecessary work processing the request further.
    ///
    /// NOTE: This does not enforce the body length!! Use [`DefaultBodyLimit`]
    /// in combination with [`axum::RequestExt::with_limited_body`] to do so.
    pub async fn check_content_length_header<B>(
        // `LayerConfig::body_limit`
        State(config_body_limit): State<Option<usize>>,
        request: http::Request<B>,
    ) -> Result<http::Request<B>, LxRejection> {
        let content_length = request
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value_str| usize::from_str(value_str).ok());

        // If a limit is configured and the header value exceeds it, reject.
        if content_length
            .zip(config_body_limit)
            .is_some_and(|(length, limit)| length > limit)
        {
            return Err(LxRejection {
                kind: LxRejectionKind::BodyLengthOverLimit,
                source_msg: "Content length header over limit".to_owned(),
            });
        }

        Ok(request)
    }
}

// --- Helpers --- //

/// Constructs a JSON [`http::Response<axum::body::Body>`] from the data and
/// status code. If serialization fails for some reason (very unlikely), log and
/// return a [`ErrorResponse`] containing a [`CommonErrorKind::Server`] along
/// with the associated [`StatusCode`].
// This is pub only bc it is used in the api_error! macro
pub fn build_json_response(
    status: StatusCode,
    data: &impl Serialize,
) -> http::Response<axum::body::Body> {
    /// Most of the logic goes in this monomorphic fn to prevent binary bloat.
    fn build_json_response_inner(
        status: StatusCode,
        try_json_bytes: Result<Vec<u8>, serde_json::Error>,
    ) -> http::Response<axum::body::Body> {
        let (status, json_bytes) = match try_json_bytes {
            Ok(jb) => (status, jb),
            Err(e) => {
                let msg = format!("Couldn't serialize response: {e:#}");
                error!(target: "http", "{msg}");
                let error_kind = CommonErrorKind::Server;
                let code = error_kind.to_code();
                let status = error_kind.to_http_status();
                let err_resp = ErrorResponse { code, msg };
                let json_bytes = serde_json::to_vec(&err_resp)
                    .expect("Serializing ErrorResponse really shouldn't fail");
                (status, json_bytes)
            }
        };

        let bytes = bytes::Bytes::from(json_bytes);
        let http_body = http_body_util::Full::new(bytes);
        let axum_body = axum::body::Body::new(http_body);

        http::Response::builder()
            .header(
                CONTENT_TYPE,
                // Can do `HeaderValue::from_static(mime::APPLICATION_JSON)`
                // if we ever have a non-trivial need for the `mime` crate
                HeaderValue::from_static("application/json"),
            )
            .status(status)
            .version(HTTP_VERSION)
            .body(axum_body)
            .expect("All operations here should be infallible")
    }

    build_json_response_inner(status, serde_json::to_vec(data))
}
