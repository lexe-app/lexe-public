use std::{future::Future, sync::Arc, time::Duration};

use axum::{Router, routing::post};
use lexe_api_core::error::BackendApiError;
use lexe_common::net;
use lexe_tokio::notify_once::NotifyOnce;
use serde::{Deserialize, Serialize};
use tracing::info_span;

use crate::{
    rest::RestClient,
    server::{self, LayerConfig, LxJson},
};

/// Spawns a TLS test server serving `router`, runs `client_fn` against its URL,
/// then gracefully shuts the server down. Returns `client_fn`'s output.
///
/// See [`do_http_request`] below for an example.
pub async fn with_test_server<F, Fut, T>(
    server_config: Arc<rustls::ServerConfig>,
    // The primary DNS name used by the server.
    server_dns: &str,
    router: Router,
    client_fn: F,
) -> T
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = T>,
{
    let shutdown = NotifyOnce::new();
    const SPAN_NAME: &str = "(test-server)";
    let (server_task, server_url) = server::spawn_server_task(
        net::LOCALHOST_WITH_EPHEMERAL_PORT,
        router,
        LayerConfig::default(),
        Some((server_config, server_dns)),
        SPAN_NAME.into(),
        info_span!(parent: None, SPAN_NAME),
        shutdown.clone(),
    )
    .expect("Failed to spawn test server");

    let output = client_fn(server_url).await;

    shutdown.send();
    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("Server shutdown timed out")
        .expect("Server task panicked");

    output
}

/// Conducts an HTTP request over TLS *with* all of our HTTP infrastructure.
/// Primarily exists to test TLS configs; may help if
/// [`crate::tls::test_utils::do_tls_handshake`] fails to reproduce an error.
pub async fn do_http_request(
    client_config: rustls::ClientConfig,
    server_config: Arc<rustls::ServerConfig>,
    server_dns: &str,
) {
    #[derive(Serialize, Deserialize)]
    struct TestRequest {
        data: String,
    }

    #[derive(Serialize, Deserialize)]
    struct TestResponse {
        data: String,
    }

    // Appends ", world" to the request data and returns the result.
    async fn handler(
        LxJson(TestRequest { mut data }): LxJson<TestRequest>,
    ) -> LxJson<TestResponse> {
        data.push_str(", world");
        LxJson(TestResponse { data })
    }

    let router = Router::new().route("/test_endpoint", post(handler));
    with_test_server(
        server_config,
        server_dns,
        router,
        |server_url| async move {
            let rest =
                RestClient::new("test-client", "test-server", client_config);
            let req = TestRequest {
                data: "hello".to_owned(),
            };
            let http_req =
                rest.post(format!("{server_url}/test_endpoint"), &req);
            let resp: TestResponse = rest
                .send::<_, BackendApiError>(http_req)
                .await
                .expect("Request failed");
            assert_eq!(resp.data, "hello, world");
        },
    )
    .await
}
