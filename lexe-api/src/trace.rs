//! This module provides API tracing utilities for both client and server,
//! including constants and fns which help keep client and server consistent.

use std::{fmt::Display, time::Duration};

use http::HeaderName;
use lexe_common::time::DisplayMs;
use lexe_crypto::rng::{RngExt, ThreadFastRng};
use lexe_logger::trace::TraceId;

/// The `target` that should be used for request spans and events.
// Short, greppable, low chance of collision in logs
pub(crate) const TARGET: &str = "lxapi";

/// The [`HeaderName`] used to read/write [`TraceId`]s.
pub(crate) static TRACE_ID_HEADER_NAME: HeaderName =
    HeaderName::from_static("lexe-trace-id");

/// Generate a random [`TraceId`] using the thread-local [`ThreadFastRng`].
pub(crate) fn gen_trace_id() -> TraceId {
    let bytes: [u8; TraceId::LENGTH] =
        ThreadFastRng::new().gen_alphanum_bytes();
    TraceId::from_alphanum_bytes(bytes).expect("Bytes are alphanumeric")
}

/// Client tracing utilities.
pub(crate) mod client {
    use tracing::info_span;

    use super::*;

    /// Get a [`tracing::Span`] and [`TraceId`] for a client request.
    pub(crate) fn request_span(
        req: &reqwest::Request,
        from: &str,
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
                let trace_id = gen_trace_id();
                request_span.record("trace_id", trace_id.as_str());

                trace_id.clone().insert_into_span(&request_span);

                trace_id
            }
        };

        (request_span, trace_id)
    }
}

/// Server tracing utilities.
pub(crate) mod server {
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
    use tracing::{debug, error, info_span, warn};

    use super::*;

    /// Builds a [`TraceLayer`] which:
    ///
    /// - Instruments each incoming request with its own request span, reusing
    ///   the [`TraceId`] from the `lexe-trace-id` header if available
    /// - Logs "Done (result)" after the completion of each response
    /// - Logs "Stream ended" when streaming bodies (post-response) complete
    /// - Logs "Other failure" whenever anything else goes wrong
    ///
    /// It can be passed to e.g. [`Router::layer`] or [`ServiceBuilder::layer`].
    ///
    /// [`Router::layer`]: axum::Router::layer
    /// [`ServiceBuilder::layer`]: tower::ServiceBuilder::layer
    pub(crate) fn trace_layer(
        api_span: tracing::Span,
        log_query_params: bool,
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
            .make_span_with(LxMakeSpan {
                api_span,
                log_query_params,
            })
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
    pub(crate) struct LxClassifyResponse;

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
    pub(crate) struct LxMakeSpan {
        /// The server API span, used as each request span's parent.
        api_span: tracing::Span,
        /// See [`LayerConfig::log_query_params`].
        ///
        /// [`LayerConfig::log_query_params`]: crate::server::LayerConfig
        log_query_params: bool,
    }

    impl<B> MakeSpan<B> for LxMakeSpan {
        fn make_span(&mut self, request: &http::Request<B>) -> tracing::Span {
            // Get the full url including query params, or just the path if
            // query param logging is disabled.
            let url = if self.log_query_params {
                request
                    .uri()
                    .path_and_query()
                    .map(|url| url.as_str())
                    .unwrap_or("/")
            } else {
                request.uri().path()
            };

            // Parse the client-provided trace id from the trace id header.
            // Generate a new trace id if none existed or if the header value
            // was invalid. There should be no need to try to get a TraceId from
            // a parent span's Extensions.
            let trace_id = request
                .headers()
                .get(&TRACE_ID_HEADER_NAME)
                .and_then(|value| TraceId::try_from(value.clone()).ok())
                .unwrap_or_else(gen_trace_id);

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
    pub(crate) struct LxOnRequest;

    impl<B> OnRequest<B> for LxOnRequest {
        fn on_request(
            &mut self,
            _request: &http::Request<B>,
            _request_span: &tracing::Span,
        ) {
            // Headers may contain sensitive data (e.g. auth tokens), so only
            // log them in tests. Requests are logged by [`LxOnResponse`].
            #[cfg(any(test, feature = "test-utils"))]
            {
                let headers = _request.headers();
                tracing::trace!(target: TARGET, ?headers, "New server request");
            }
        }
    }

    /// [`OnResponse`] impl which logs the completion of requests by the server.
    /// `RestClient` logs `req_time`; analogously here we log `resp_time`.
    ///
    /// The "Done" event is the single per-request log line; its request span
    /// carries `trace_id`, `from`, `method` and `url`.
    #[derive(Clone)]
    pub(crate) struct LxOnResponse;

    impl<B> OnResponse<B> for LxOnResponse {
        fn on_response(
            self,
            response: &http::Response<B>,
            // Client logs "req_time", server logs "resp_time"
            resp_time: Duration,
            _request_span: &tracing::Span,
        ) {
            let status = response.status();
            let resp_time = DisplayMs(resp_time);

            if status.is_success() {
                debug!(target: TARGET, %resp_time, ?status, "Done (success)");
            } else if status.is_client_error() {
                warn!(target: TARGET, %resp_time, ?status, "Done (client error)");
            } else if status.is_server_error() && status.as_u16() == 503 {
                // Don't spam ERRORs for 503 "Service Unavailable"s which we
                // return when load-shedding requests. ERRORs should be serious.
                warn!(target: TARGET, %resp_time, ?status, "Done (unavailable)");
            } else if status.is_server_error() {
                error!(target: TARGET, %resp_time, ?status, "Done (server error)");
            } else {
                debug!(target: TARGET, %resp_time, ?status, "Done (other)");
            }

            // Headers may contain sensitive data, so only log them in tests.
            #[cfg(any(test, feature = "test-utils"))]
            tracing::trace!(
                target: TARGET, %resp_time, ?status,
                headers = ?response.headers(),
                "Done (headers)",
            );
        }
    }

    /// Basic [`OnEos`] impl; we don't stream atm but this will work if we do
    #[derive(Clone)]
    pub(crate) struct LxOnEos;

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
            debug!(target: TARGET, %stream_time, ?num_trailers, "Stream ended");
        }
    }

    /// [`OnFailure`] impl which logs failures. Since [`LxClassifyResponse`]
    /// does not classify any responses (including error responses) or
    /// end-of-streams as failures, this will trigger only if the inner
    /// [`tower::Service`] future resolves to an error, or if `Body::poll_frame`
    /// returns an error. See [`tower_http::trace`] module docs for more info.
    #[derive(Clone)]
    pub(crate) struct LxOnFailure;

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
