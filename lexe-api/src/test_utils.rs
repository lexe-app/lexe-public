use std::{sync::Arc, time::Duration};

use axum::{routing::post, Router};
use common::net;
use lexe_api_core::error::BackendApiError;
use lexe_tokio::notify_once::NotifyOnce;
use serde::{Deserialize, Serialize};
use tracing::info_span;

use crate::{
    rest::RestClient,
    server::{self, LayerConfig, LxJson},
};

/// Conducts an HTTP request over TLS *with* all of our HTTP infrastructure.
/// Primarily exists to test TLS configs; may help if
/// [`crate::tls::test_utils::do_tls_handshake`] fails to reproduce an error.
pub async fn do_http_request(
    client_config: rustls::ClientConfig,
    server_config: Arc<rustls::ServerConfig>,
    // The DNS name used to reach the server.
    server_dns: &str,
) {
    // Request/response structs and handler used by `do_tls_handshake_with_http`
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
    let shutdown = NotifyOnce::new();
    let tls_and_dns = Some((server_config, server_dns));
    const TEST_SPAN_NAME: &str = "(test-server)";
    let (server_task, server_url) = server::spawn_server_task(
        net::LOCALHOST_WITH_EPHEMERAL_PORT,
        router,
        LayerConfig::default(),
        tls_and_dns,
        TEST_SPAN_NAME.into(),
        info_span!(parent: None, TEST_SPAN_NAME),
        shutdown.clone(),
    )
    .expect("Failed to spawn test server");

    let rest = RestClient::new("test-client", "test-server", client_config);
    let req = TestRequest {
        data: "hello".to_owned(),
    };
    let http_req = rest.post(format!("{server_url}/test_endpoint"), &req);
    let resp: TestResponse = rest
        .send::<_, BackendApiError>(http_req)
        .await
        .expect("Request failed");
    assert_eq!(resp.data, "hello, world");

    shutdown.send();
    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("Server shutdown timed out")
        .expect("Server task panicked");
}
