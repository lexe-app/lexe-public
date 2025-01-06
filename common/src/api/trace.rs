//! This module provides API tracing utilities for both client and server,
//! including constants and fns which help keep client and server consistent.

use std::{
    fmt::{self, Display},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{bail, ensure, Context};
use http::{HeaderName, HeaderValue};
use rand_core::RngCore;
use tracing::{span, warn, Dispatch};

#[cfg(doc)]
use crate::api::rest::RestClient;
use crate::rng::ThreadWeakRng;

/// The `target` that should be used for request spans and events.
// Short, greppable, low chance of collision in logs
pub(crate) const TARGET: &str = "lxapi";

/// The [`HeaderName`] used to read/write [`TraceId`]s.
pub(crate) static TRACE_ID_HEADER_NAME: HeaderName =
    HeaderName::from_static("lexe-trace-id");

/// A [`TraceId`] identifies a tree of requests sharing a single causal source
/// as it travels between different Lexe services.
/// - It is generated by the originating client and propagated via HTTP headers
///   between services, and via tracing span `Extensions` within services.
/// - Each [`TraceId`] consists of 16 alphanumeric bytes (a-z,A-Z,0-9); all
///   [`TraceId`] constructors must enforce this invariant.
#[derive(Clone, PartialEq)]
pub struct TraceId(HeaderValue);

/// A function pointer to a fn which attempts to extract a [`TraceId`] from the
/// `Extensions` of the given span or any of its parents.
///
/// This static is required because we are required to downcast the [`Dispatch`]
/// to a concrete subscriber type (which implements the `LookupSpan` trait),
/// which we do not know since we are agnostic over [`tracing::Subscriber`]s.
/// An alternative was to make the [`RestClient`] generic over a logger `L` so
/// that the subscriber type was known when calling `client::request_span`.
/// However, this results in highly undesirable [`RestClient`] ergonomics.
/// Instead, we provide impls for these fns via the `define_trace_id_fns` macro,
/// and initialize these statics in the `try_init()` of our loggers.
pub static GET_TRACE_ID_FN: OnceLock<
    fn(&span::Id, &Dispatch) -> anyhow::Result<Option<TraceId>>,
> = OnceLock::new();
/// Like [`GET_TRACE_ID_FN`], but inserts a [`TraceId`] into the `Extensions`
/// of a given span, returning the replaced [`TraceId`] if it existed.
pub static INSERT_TRACE_ID_FN: OnceLock<
    fn(&span::Id, &Dispatch, TraceId) -> anyhow::Result<Option<TraceId>>,
> = OnceLock::new();

impl TraceId {
    /// Byte length of a [`TraceId`].
    const LENGTH: usize = 16;

    /// Convenience to generate a [`TraceId`] using the thread-local
    /// [`ThreadWeakRng`].
    pub fn generate() -> Self {
        Self::from_rng(&mut ThreadWeakRng::new())
    }

    /// Generate a [`TraceId`] from an existing rng.
    pub fn from_rng(rng: &mut impl RngCore) -> Self {
        use crate::rng::RngExt;

        // Generate a 16 byte array with alphanumeric characters
        let buf: [u8; Self::LENGTH] = rng.gen_alphanum_bytes();

        let header_value = HeaderValue::from_bytes(&buf).expect(
            "All alphanumeric bytes are in range (32..=255), \
             and none are byte 127 (DEL). This is also checked in tests.",
        );

        Self(header_value)
    }

    /// Get this [`TraceId`] as a [`&str`].
    pub fn as_str(&self) -> &str {
        debug_assert!(std::str::from_utf8(self.0.as_bytes()).is_ok());
        // SAFETY: All constructors ensure that all bytes are alphanumeric
        unsafe { std::str::from_utf8_unchecked(self.0.as_bytes()) }
    }

    /// Get a corresponding [`HeaderValue`].
    pub fn to_header_value(&self) -> HeaderValue {
        self.0.clone()
    }

    /// Get the [`TraceId`] from the `Extensions` of the given span or any of
    /// its parents, logging any errors that occur as warnings.
    fn get_from_span(span: &tracing::Span) -> Option<Self> {
        // Tests are usually not instrumented with tracing spans. To prevent
        // tests from spamming "WARN: Span is not enabled", we return early if
        // the given span was disabled and we are in test. In prod, however,
        // ~everything should be instrumented, so we do want to log the `WARN`s.
        #[cfg(any(test, feature = "test-utils"))]
        if span.is_disabled() {
            return None;
        }

        let try_get_trace_id = || {
            // Fetch the `get_trace_id_from_span` fn pointer from the static.
            let get_trace_id_fn = GET_TRACE_ID_FN.get().context(
                "GET_TRACE_ID_FN not set. Did logger::try_init() \
                 initialize the TraceId statics?",
            )?;
            let maybe_trace_id = span
                // Here, we actually call the fn to try to get the trace id.
                .with_subscriber(|(id, dispatch)| get_trace_id_fn(id, dispatch))
                .context("Span is not enabled")?
                .context("get_trace_id_fn (get_trace_id_from_span) failed")?;

            Ok::<_, anyhow::Error>(maybe_trace_id)
        };

        try_get_trace_id()
            .inspect_err(|e| warn!("Failed to check for trace id: {e:#}"))
            .unwrap_or_default()
    }

    /// Insert this [`TraceId`] into the `Extensions` of the given span,
    /// logging any errors that occur as warnings. Also logs a warning if a
    /// [`TraceId`] already existed in the span and was replaced by this insert.
    fn insert_into_span(self, span: &tracing::Span) {
        let try_insert_trace_id = || {
            // Fetch the `insert_trace_id_into_span` fn pointer from the static.
            let insert_trace_id_fn = INSERT_TRACE_ID_FN.get().context(
                "INSERT_TRACE_ID_FN not set. Did logger::try_init() \
                 initialize the TraceId statics?",
            )?;

            let maybe_replaced = span
                // Here, we actually call the fn to try to insert the trace id.
                .with_subscriber(|(id, dispatch)| {
                    insert_trace_id_fn(id, dispatch, self)
                })
                .context("Span is not enabled")?
                .context("insert_trace_id_into_span failed")?;

            Ok::<_, anyhow::Error>(maybe_replaced)
        };

        try_insert_trace_id()
            .inspect_err(|e| warn!("Failed to insert trace id: {e:#}"))
            .unwrap_or_default()
            .inspect(|replaced| warn!("Replaced existing TraceId: {replaced}"));
    }

    /// A test implementation which can be used to test that getting and setting
    /// [`TraceId`]s from and into span `Extensions` works with a specific
    /// [`Subscriber`]. Call it after logger init like so:
    ///
    /// ```ignore
    /// #[test]
    /// fn get_and_insert_trace_ids() {
    ///     let _ = try_init();
    ///     TraceId::get_and_insert_test_impl();
    /// }
    /// ```
    ///
    /// NOTE: This impl downcasts [`tracing::Dispatch`] to the subscriber type
    /// specified during logger init, which fails if the global logger has been
    /// set to a different [`Subscriber`] type than the one expected in this
    /// test, which may happen if multiple tests are run in parallel in the same
    /// process with different [`Subscriber`]s. Thankfully, cargo test builds
    /// separate test binaries for each crate and then runs each (serially) as
    /// its own process. This should usually prevent conflicts since each crate
    /// usually only uses one [`Subscriber`] type, but if a test starts flaking
    /// because of this, feel free to just `#[ignore]` the test, and manually
    /// run the test only when making changes to the logging / tracing setup.
    ///
    /// [`Subscriber`]: tracing::Subscriber
    #[cfg(any(test, feature = "test-utils"))]
    pub fn get_and_insert_test_impl() {
        use tracing::{error_span, info};

        GET_TRACE_ID_FN.get().expect("GET_TRACE_ID_FN not set");
        INSERT_TRACE_ID_FN
            .get()
            .expect("INSERT_TRACE_ID_FN not set");

        let trace_id1 = TraceId::generate();

        // Use error spans so that the test still passes with `RUST_LOG=error`.
        let outer_span = error_span!("(outer)", trace_id=%trace_id1);

        // Sanity check: get_from_span should return no TraceId.
        assert!(TraceId::get_from_span(&outer_span).is_none());

        // Insert the TraceId into the outer span's extensions.
        trace_id1.clone().insert_into_span(&outer_span);

        // Enter the outer span.
        outer_span.in_scope(|| {
            info!("This msg should contain (outer) and `trace_id`");

            // We should be able to recover the trace id within the outer span.
            let current_span = tracing::Span::current();
            let trace_id2 = TraceId::get_from_span(&current_span)
                .expect("No trace id returned");
            assert_eq!(trace_id1, trace_id2);

            // Create an inner span. trace_id should've been set by the parent.
            let inner_span =
                error_span!("(inner)", trace_id = tracing::field::Empty);

            // Enter the inner span.
            inner_span.in_scope(|| {
                info!("This msg should have (outer):(inner) and `trace_id`");

                // Should be able to recover the trace id within the inner span.
                let current_span = tracing::Span::current();
                let trace_id3 = TraceId::get_from_span(&current_span)
                    .expect("No trace id returned");
                assert_eq!(trace_id2, trace_id3);
            });
        });

        info!("Test complete");
    }
}

impl TryFrom<HeaderValue> for TraceId {
    type Error = anyhow::Error;

    fn try_from(src: HeaderValue) -> Result<Self, Self::Error> {
        let src_bytes = src.as_bytes();
        if src_bytes.len() != Self::LENGTH {
            bail!("Source header value had wrong length");
        }

        let all_alphanumeric = src_bytes
            .iter()
            .all(|byte| char::is_alphanumeric(*byte as char));
        ensure!(
            all_alphanumeric,
            "Source header value contained non-alphanumeric bytes"
        );

        Ok(Self(src))
    }
}

impl Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Debug for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::rng::WeakRng;

    impl Arbitrary for TraceId {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<WeakRng>()
                .prop_map(|mut rng| Self::from_rng(&mut rng))
                .boxed()
        }
    }
}

/// Generates implementations of the functions pointed to by [`GET_TRACE_ID_FN`]
/// and [`INSERT_TRACE_ID_FN`] given the type of the [`tracing::Subscriber`].
/// The caller (typically `logger::try_init()`) is responsible for initializing
/// these statics using the generated implementations.
///
/// ```ignore
/// # use anyhow::{anyhow, Context};
/// # use tracing_subscriber::{util::SubscriberInitExt, FmtSubscriber};
/// #
/// pub fn try_init() -> anyhow::Result<()> {
///     FmtSubscriber::new().try_init().context("Logger already set")?;
///
///     // Notice how `FmtSubscriber` here exactly matches our subscriber type.
///     // If using a more complex subscriber, you will have to name the type,
///     // e.g. `Layered<Filtered<FmtLayer<Registry, ...>, ..., ...>, ...>`.
///     // See public/logger/src/lib.rs for an example of this.
///     common::define_trace_id_fns!(FmtSubscriber);
///     common::api::trace::GET_TRACE_ID_FN
///         .set(get_trace_id_from_span)
///         .map_err(|_| anyhow!("GET_TRACE_ID_FN already set"))?;
///     common::api::trace::INSERT_TRACE_ID_FN
///         .set(insert_trace_id_into_span)
///         .map_err(|_| anyhow!("INSERT_TRACE_ID_FN already set"))?;
///
///     Ok(())
/// }
/// ```
#[macro_export]
macro_rules! define_trace_id_fns {
    ($subscriber:ty) => {
        use anyhow::Context;
        use common::api::trace::TraceId;
        use tracing_subscriber::registry::LookupSpan;

        /// Get the [`TraceId`] from the `Extensions` of this span or any of its
        /// parents. Errors if downcasting to the subscriber fails, or if the
        /// subscriber doesn't return a `SpanRef` for the given span id.
        fn get_trace_id_from_span(
            id: &tracing::span::Id,
            dispatch: &tracing::Dispatch,
        ) -> anyhow::Result<Option<TraceId>> {
            let subscriber = dispatch.downcast_ref::<$subscriber>().context(
                "Downcast failed. Did logger::try_init() define the trace_id \
                 fns with the correct subscriber type?",
            )?;
            let span_ref = subscriber
                .span(id)
                .context("Failed to get SpanRef from id")?;
            let maybe_trace_id = span_ref
                .scope()
                .find_map(|span| span.extensions().get::<TraceId>().cloned());
            Ok(maybe_trace_id)
        }

        /// Insert the [`TraceId`] into the `Extensions` of the given span.
        /// Errors if downcasting to the subscriber fails, or if the
        /// subscriber doesn't return a `SpanRef` for the given span id.
        /// Returns the replaced [`TraceId`] if one already existed in the span.
        fn insert_trace_id_into_span(
            id: &tracing::span::Id,
            dispatch: &tracing::Dispatch,
            trace_id: TraceId,
        ) -> anyhow::Result<Option<TraceId>> {
            let subscriber = dispatch.downcast_ref::<$subscriber>().context(
                "Downcast failed. Did logger::try_init() define the trace_id \
                 fns with the correct subscriber type?",
            )?;
            let span_ref = subscriber.span(id).context("No span ref for id")?;
            let maybe_replaced = span_ref.extensions_mut().replace(trace_id);
            Ok(maybe_replaced)
        }
    };
}

/// [`Display`]s a [`Duration`] in ms with 3 decimal places, e.g. "123.456ms".
/// Used to log request / response times in a consistent unit.
pub(crate) struct DisplayMs(pub Duration);

impl Display for DisplayMs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.0.as_secs_f64() * 1000.0;
        write!(f, "{ms:.3}ms")
    }
}

/// Client tracing utilities.
pub mod client {
    use tracing::info_span;

    use super::*;

    /// Get a [`tracing::Span`] and [`TraceId`] for a client request.
    pub fn request_span(
        req: &reqwest::Request,
        from: &'static str,
        to: &'static str,
    ) -> (tracing::Span, TraceId) {
        // Our client request span.
        let request_span = info_span!(
            target: TARGET,
            "(req)(cli)",
            // This does not overwrite existing values if the field was already
            // set by a parent span. If it was not set, we will record it below.
            trace_id = tracing::field::Empty,
            %from,
            %to,
            method = %req.method(),
            url = %req.url(),
            // This is set later in the request
            attempts_left = tracing::field::Empty,
        );

        // Check if a parent span already has a trace_id set in its Extensions,
        // and extract it if so. This happens when an outgoing client request is
        // made in the API handler of a server which has created a (server-side)
        // request span for a (client) request which included a trace id header.
        let existing_trace_id =
            TraceId::get_from_span(&tracing::Span::current());

        // If we found a TraceId from a parent span, the parent will already
        // have set the trace_id field and included it in its Extensions, so
        // there is nothing to do. Otherwise, we need to generate a new TraceId,
        // record it in the trace_id field, and add it our span's Extensions.
        // NOTE: Setting the trace_id field here after it has already been set
        // by a parent will result in duplicate trace_id keys in logs.
        let trace_id = match existing_trace_id {
            Some(tid) => tid,
            None => {
                let trace_id = TraceId::generate();
                request_span.record("trace_id", trace_id.as_str());

                trace_id.clone().insert_into_span(&request_span);

                trace_id
            }
        };

        (request_span, trace_id)
    }
}

/// Server tracing utilities.
pub mod server {
    use anyhow::anyhow;
    use http::header::USER_AGENT;
    use tower_http::{
        classify::{
            ClassifiedResponse, ClassifyResponse, NeverClassifyEos,
            SharedClassifier,
        },
        trace::{
            MakeSpan, OnEos, OnFailure, OnRequest, OnResponse, TraceLayer,
        },
    };
    use tracing::{debug, error, info, info_span, warn};

    use super::*;

    /// Builds a [`TraceLayer`] which:
    ///
    /// - Instruments each incoming request with its own request span, reusing
    ///   the [`TraceId`] from the `lexe-trace-id` header if available
    /// - Logs "New server request" at the start of each received request
    /// - Logs "Done (result) after the completion of each response"
    /// - Logs "Stream ended" when streaming bodies (post-response) complete
    /// - Logs "Other failure" whenever anything else goes wrong
    ///
    /// It can be passed to e.g. [`Router::layer`] or [`ServiceBuilder::layer`].
    ///
    /// [`Router::layer`]: axum::Router::layer
    /// [`ServiceBuilder::layer`]: tower::ServiceBuilder::layer
    pub fn trace_layer(
        api_span: tracing::Span,
    ) -> TraceLayer<
        SharedClassifier<LxClassifyResponse>,
        LxMakeSpan,
        LxOnRequest,
        LxOnResponse,
        (),
        LxOnEos,
        LxOnFailure,
    > {
        // `tower_http::trace` documents when each of these callbacks is called.
        TraceLayer::new(SharedClassifier::new(LxClassifyResponse))
            .make_span_with(LxMakeSpan { api_span })
            .on_request(LxOnRequest)
            .on_response(LxOnResponse)
            // Do nothing on body chunk
            .on_body_chunk(())
            .on_eos(LxOnEos)
            .on_failure(LxOnFailure)
    }

    /// A [`ClassifyResponse`] which classifies all responses as OK.
    ///
    /// We do this because all responses (including error responses) are already
    /// logged by [`LxOnResponse`], so triggering [`OnFailure`] on error status
    /// codes (the default impl for HTTP) would result in redundant logs.
    #[derive(Clone)]
    pub struct LxClassifyResponse;

    impl ClassifyResponse for LxClassifyResponse {
        type FailureClass = anyhow::Error;
        type ClassifyEos = NeverClassifyEos<Self::FailureClass>;

        fn classify_response<B>(
            self,
            _response: &http::Response<B>,
        ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
            ClassifiedResponse::Ready(Ok(()))
        }

        fn classify_error<E: Display + 'static>(
            self,
            error: &E,
        ) -> Self::FailureClass {
            anyhow!("{error:#}")
        }
    }

    /// A [`MakeSpan`] impl which mirrors [`client::request_span`].
    #[derive(Clone)]
    pub struct LxMakeSpan {
        /// The server API span, used as each request span's parent.
        api_span: tracing::Span,
    }

    impl<B> MakeSpan<B> for LxMakeSpan {
        fn make_span(&mut self, request: &http::Request<B>) -> tracing::Span {
            // Get the full url, including query params
            let url = request
                .uri()
                .path_and_query()
                .map(|url| url.as_str())
                .unwrap_or("/");

            // Parse the client-provided trace id from the trace id header.
            // Generate a new trace id if none existed or if the header value
            // was invalid. There should be no need to try to get a TraceId from
            // a parent span's Extensions.
            let trace_id = request
                .headers()
                .get(&TRACE_ID_HEADER_NAME)
                .and_then(|value| TraceId::try_from(value.clone()).ok())
                .unwrap_or_else(TraceId::generate);

            // Log the user agent header as `from` if it exists, or a default.
            // This is the same `from` as in the RestClient and client span.
            // The `to` field does not need to be logged since it is already
            // included as part of the server's span name.
            let from = request
                .headers()
                .get(USER_AGENT)
                .map(|value| value.to_str().unwrap_or("(non-ascii)"))
                .unwrap_or("(none)");

            let request_span = info_span!(
                target: TARGET,
                parent: self.api_span.clone(),
                "(req)(srv)",
                %trace_id,
                %from,
                method = %request.method().as_str(),
                url = %url,
                version = ?request.version(),
            );

            // Insert the trace id into the server request span's `Extensions`,
            // so that any client requests made in our handler can pick it up.
            trace_id.insert_into_span(&request_span);

            request_span
        }
    }

    /// `OnRequest` impl mirroring `RestClient::send_inner`.
    #[derive(Clone)]
    pub struct LxOnRequest;

    impl<B> OnRequest<B> for LxOnRequest {
        fn on_request(
            &mut self,
            request: &http::Request<B>,
            _request_span: &tracing::Span,
        ) {
            let headers = request.headers();
            debug!(target: TARGET, "New server request");
            debug!(target: TARGET, ?headers, "Server request (headers)");
        }
    }

    /// [`OnResponse`] impl which logs the completion of requests by the server.
    /// `RestClient` logs `req_time`; analogously here we log `resp_time`.
    #[derive(Clone)]
    pub struct LxOnResponse;

    impl<B> OnResponse<B> for LxOnResponse {
        fn on_response(
            self,
            response: &http::Response<B>,
            // Client logs "req_time", server logs "resp_time"
            resp_time: Duration,
            _request_span: &tracing::Span,
        ) {
            let status = response.status();
            let headers = response.headers();
            let resp_time = DisplayMs(resp_time);

            if status.is_success() {
                info!(target: TARGET, %resp_time, ?status, "Done (success)");
            } else if status.is_client_error() {
                warn!(target: TARGET, %resp_time, ?status, "Done (client error)");
            } else if status.is_server_error() && status.as_u16() == 503 {
                // Don't spam ERRORs for 503 "Service Unavailable"s which we
                // return when load-shedding requests. ERRORs should be serious.
                warn!(target: TARGET, %resp_time, ?status, "Done (load shedded)");
            } else if status.is_server_error() {
                error!(target: TARGET, %resp_time, ?status, "Done (server error)");
            } else {
                info!(target: TARGET, %resp_time, ?status, "Done (other)");
            }

            // Log the headers too, but only at DEBUG.
            debug!(
                target: TARGET, %resp_time, ?status, ?headers,
                "Done (headers)",
            );
        }
    }

    /// Basic [`OnEos`] impl; we don't stream atm but this will work if we do
    #[derive(Clone)]
    pub struct LxOnEos;

    impl OnEos for LxOnEos {
        fn on_eos(
            self,
            trailers: Option<&http::HeaderMap>,
            // The duration since the response was sent
            stream_time: Duration,
            _request_span: &tracing::Span,
        ) {
            let num_trailers = trailers.map(|trailers| trailers.len());
            let stream_time = DisplayMs(stream_time);
            info!(target: TARGET, %stream_time, ?num_trailers, "Stream ended");
        }
    }

    /// [`OnFailure`] impl which logs failures. Since [`LxClassifyResponse`]
    /// does not classify any responses (including error responses) or
    /// end-of-streams as failures, this will trigger only if the inner
    /// [`tower::Service`] future resolves to an error, or if `Body::poll_frame`
    /// returns an error. See [`tower_http::trace`] module docs for more info.
    #[derive(Clone)]
    pub struct LxOnFailure;

    impl<FailureClass: Display> OnFailure<FailureClass> for LxOnFailure {
        fn on_failure(
            &mut self,
            fail_class: FailureClass,
            // The duration since the request was received
            fail_time: Duration,
            _request_span: &tracing::Span,
        ) {
            let fail_time = DisplayMs(fail_time);
            warn!(target: TARGET, %fail_time, %fail_class, "Other failure");
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn trace_id_proptest() {
        // TraceId's Arbitrary impl uses TraceId::from_rng
        proptest!(|(id1: TraceId)| {
            // Ensure the debug_assert! in TraceId::as_str() doesn't panic
            id1.as_str();
            // TraceId -> HeaderValue -> TraceId
            let id2 = TraceId::try_from(id1.to_header_value()).unwrap();
            prop_assert_eq!(&id1, &id2);
        });
    }
}
