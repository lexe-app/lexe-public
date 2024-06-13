use std::sync::Arc;

use lazy_lock::LazyLock;
use rcgen::{DistinguishedName, DnType};
use rustls::{crypto::WebPkiSupportedAlgorithms, ClientConfig, ServerConfig};

use crate::ed25519;

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// Certs and utilities related to Lexe's CA.
pub mod lexe_ca;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;
/// TLS newtypes, namely DER-encoded certs and cert keys.
pub mod types;

/// Allow accessing [`rustls`] via `common::tls`
pub use rustls;

/// Whether the given DER-encoded cert bound to the given DNS name.
/// Returns [`false`] if the certificate failed to parse.
#[must_use]
pub fn cert_contains_dns(cert_der: &[u8], expected_dns: &str) -> bool {
    // Fake keypair which isn't actually used for validation
    let fake_keypair = ed25519::KeyPair::from_seed(&[69; 32]).to_rcgen();

    // This method is ostensibly for CA certs, but doesn't actually check if
    // the cert is a CA cert, so it should be fine to reuse here
    let cert_params = match rcgen::CertificateParams::from_ca_cert_der(
        cert_der,
        fake_keypair,
    ) {
        Ok(params) => params,
        Err(_) => return false,
    };

    cert_params.subject_alt_names.iter().any(|san_type| {
        matches!(
            san_type,
            rcgen::SanType::DnsName(bound_dns) if bound_dns == expected_dns
        )
    })
}

/// Our [`rustls::crypto::CryptoProvider`].
/// Use this instead of [`rustls::crypto::ring::default_provider`].
pub static LEXE_CRYPTO_PROVIDER: LazyLock<Arc<rustls::crypto::CryptoProvider>> =
    LazyLock::new(|| {
        #[allow(clippy::disallowed_methods)] // We customize it here
        let mut provider = rustls::crypto::ring::default_provider();
        LEXE_CIPHER_SUITES.clone_into(&mut provider.cipher_suites);
        LEXE_KEY_EXCHANGE_GROUPS.clone_into(&mut provider.kx_groups);
        provider.signature_verification_algorithms = LEXE_SIGNATURE_ALGORITHMS;
        // provider.secure_random = &Ring;
        // provider.key_provider = &Ring;
        Arc::new(provider)
    });

/// The value to pass to
/// [`ServerCertVerifier::supported_verify_schemes`](rustls::client::danger::ServerCertVerifier::supported_verify_schemes)
pub static LEXE_SUPPORTED_VERIFY_SCHEMES: LazyLock<
    Vec<rustls::SignatureScheme>,
> = LazyLock::new(|| {
    LEXE_SIGNATURE_ALGORITHMS
        .mapping
        .iter()
        .map(|(sigscheme, _sig_verify_alg)| *sigscheme)
        .collect()
});

/// Lexe signature algorithms: Only Ed25519.
/// Pass this to [`rustls::crypto::verify_tls13_signature`].
pub static LEXE_SIGNATURE_ALGORITHMS: WebPkiSupportedAlgorithms =
    WebPkiSupportedAlgorithms {
        all: &[rustls_webpki::ring::ED25519],
        mapping: &[(
            rustls::SignatureScheme::ED25519,
            &[rustls_webpki::ring::ED25519],
        )],
    };

/// Lexe TLS protocol version: TLSv1.3
static LEXE_TLS_PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] =
    &[&rustls::version::TLS13];
/// Lexe cipher suite: specifically `TLS13_AES_128_GCM_SHA256`
static LEXE_CIPHER_SUITES: &[rustls::SupportedCipherSuite] =
    &[rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256];
/// Lexe key exchange group: X25519
static LEXE_KEY_EXCHANGE_GROUPS: &[&dyn rustls::crypto::SupportedKxGroup] =
    &[rustls::crypto::ring::kx_group::X25519];

/// Lexe default value for [`ClientConfig::alpn_protocols`] and
/// [`ServerConfig::alpn_protocols`]: HTTP/1.1 and HTTP/2
pub static LEXE_ALPN_PROTOCOLS: LazyLock<Vec<Vec<u8>>> =
    LazyLock::new(|| vec!["h2".into(), "http/1.1".into()]);
/// A safe default for [`rcgen::CertificateParams::subject_alt_names`] when
/// there isn't a specific value that makes sense. Used for client / CA certs.
pub static DEFAULT_SUBJECT_ALT_NAMES: LazyLock<Vec<rcgen::SanType>> =
    LazyLock::new(|| vec![rcgen::SanType::DnsName("lexe.app".to_owned())]);

/// Helper to get a builder for a [`ClientConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn client_config_builder(
) -> rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier> {
    // We use the correct provider and TLS versions here
    #[allow(clippy::disallowed_methods)]
    ClientConfig::builder_with_provider(LEXE_CRYPTO_PROVIDER.clone())
        .with_protocol_versions(LEXE_TLS_PROTOCOL_VERSIONS)
        .expect("Checked in tests")
}

/// Helper to get a builder for a [`ServerConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn server_config_builder(
) -> rustls::ConfigBuilder<ServerConfig, rustls::WantsVerifier> {
    // We use the correct provider and TLS versions here
    #[allow(clippy::disallowed_methods)]
    ServerConfig::builder_with_provider(LEXE_CRYPTO_PROVIDER.clone())
        .with_protocol_versions(LEXE_TLS_PROTOCOL_VERSIONS)
        .expect("Checked in tests")
}

/// Build a [`rcgen::Certificate`] with Lexe presets and optional overrides.
/// - This builder function helps ensure that important fields in the inner
///   [`rcgen::CertificateParams`] struct are considered.
/// - Specify any special fields or overrides with the `overrides` closure.
/// - `key_pair` and `alg` cannot be overridden.
///
/// # Example
///
/// ```
/// # use common::ed25519;
/// # use rcgen::{IsCa, BasicConstraints};
/// let key_pair = ed25519::KeyPair::from_seed(&[69; 32]);
/// let cert = common::tls::build_rcgen_cert(
///     "My Lexe cert common name",
///     rcgen::date_time_ymd(1975, 1, 1),
///     rcgen::date_time_ymd(4096, 1, 1),
///     vec![rcgen::SanType::DnsName("localhost".to_owned())],
///     &key_pair,
///     |params: &mut rcgen::CertificateParams| {
///         params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
///     },
/// );
/// ```
pub fn build_rcgen_cert(
    common_name: &str,
    not_before: time::OffsetDateTime,
    not_after: time::OffsetDateTime,
    subject_alt_names: Vec<rcgen::SanType>,
    key_pair: &ed25519::KeyPair,
    overrides: impl FnOnce(&mut rcgen::CertificateParams),
) -> rcgen::Certificate {
    let mut params = rcgen::CertificateParams::default();

    // alg (set below)
    params.not_before = not_before;
    params.not_after = not_after;
    // serial_number: None
    params.subject_alt_names = subject_alt_names;
    params.distinguished_name = lexe_distinguished_name(common_name);
    // is_ca: IsCa::NoCa,
    // key_usages: Vec::new(),
    // extended_key_usages: Vec::new(),
    // name_constraints: None,
    // custom_extensions: Vec::new(),
    // key_pair (set below)
    // use_authority_key_identifier_extension: false,
    // key_identifier_method: KeyIdMethod::Sha256,

    overrides(&mut params);

    // Prevent these from being overridden to make panics impossible
    params.alg = &rcgen::PKCS_ED25519;
    params.key_pair = Some(key_pair.to_rcgen());

    rcgen::Certificate::from_params(params)
        .expect("Can only panic if algorithm doesn't match keypair")
}

/// Build a Lexe Distinguished Name given a Common Name.
pub fn lexe_distinguished_name(common_name: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-app");
    name.push(DnType::CommonName, common_name);
    name
}

/// TLS-specific test utilities.
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils {
    use std::{sync::Arc, time::Duration};

    use anyhow::Context;
    use axum::{routing::post, Router};
    use rustls::pki_types::ServerName;
    use serde::{Deserialize, Serialize};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tracing::info_span;

    use super::*;
    use crate::{
        api::{
            self,
            error::BackendApiError,
            rest::RestClient,
            server::{LayerConfig, LxJson},
        },
        net,
        shutdown::ShutdownChannel,
    };

    /// Conducts a TLS handshake without any other [`reqwest`]/[`axum`] infra,
    /// over a fake pair of connected streams. Returns the client and server
    /// results instead of panicking so that negative cases can be tested too.
    pub async fn do_tls_handshake(
        client_config: Arc<ClientConfig>,
        server_config: Arc<ServerConfig>,
        // This is the DNS name that the *client* expects the server to have.
        expected_dns: String,
    ) -> [Result<(), String>; 2] {
        // a fake pair of connected streams
        let (client_stream, server_stream) = tokio::io::duplex(4096);

        // client connects, sends "hello", receives "goodbye"
        let client = async move {
            let connector = tokio_rustls::TlsConnector::from(client_config);
            let sni = ServerName::try_from(expected_dns).unwrap();
            let mut stream = connector
                .connect(sni, client_stream)
                .await
                .context("Client didn't connect")?;

            // client: >> send "hello"
            stream
                .write_all(b"hello")
                .await
                .context("Could not write hello")?;
            stream.flush().await.context("Toilet clogged")?;
            stream.shutdown().await.context("Could not shutdown")?;

            // client: << recv "goodbye"
            let mut resp = Vec::new();
            stream.read_to_end(&mut resp).await.context("Read failed")?;
            assert_eq!(&resp, b"goodbye");

            Ok::<_, anyhow::Error>(())
        };

        // server accepts, receives "hello", responds with "goodbye"
        let server = async move {
            let acceptor = tokio_rustls::TlsAcceptor::from(server_config);
            let mut stream = acceptor
                .accept(server_stream)
                .await
                .context("Server didn't accept")?;

            // server: >> recv "hello"
            let mut req = Vec::new();
            stream.read_to_end(&mut req).await.context("Read failed")?;
            assert_eq!(&req, b"hello");

            // server: << send "goodbye"
            stream
                .write_all(b"goodbye")
                .await
                .context("Could not write goodbye")?;
            stream.shutdown().await.context("Could not shutdown")?;

            Ok::<_, anyhow::Error>(())
        };

        let (client_result, server_result) = tokio::join!(client, server);

        // Convert `anyhow::Error`s to strings for better ergonomics downstream
        let (client_result, server_result) = (
            client_result.map_err(|e| format!("{e:#}")),
            server_result.map_err(|e| format!("{e:#}")),
        );

        println!("Client result: {client_result:?}");
        println!("Server result: {server_result:?}");
        println!("---");

        [client_result, server_result]
    }

    /// Conducts an HTTP request over TLS *with* all of our HTTP infrastructure.
    /// May help if [`do_tls_handshake`] fails to reproduce an error.
    pub async fn do_http_request(
        client_config: ClientConfig,
        server_config: Arc<ServerConfig>,
        // The DNS name used to reach the server.
        server_dns: &str,
    ) {
        let router = Router::new().route("/test_endpoint", post(handler));
        let shutdown = ShutdownChannel::new();
        let tls_and_dns = Some((server_config, server_dns));
        const TEST_SPAN_NAME: &str = "(test-server)";
        let (server_task, server_url) = api::server::spawn_server_task(
            net::LOCALHOST_WITH_EPHEMERAL_PORT,
            router,
            LayerConfig::default(),
            tls_and_dns,
            TEST_SPAN_NAME,
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
    #[axum::debug_handler]
    async fn handler(
        LxJson(TestRequest { mut data }): LxJson<TestRequest>,
    ) -> LxJson<TestResponse> {
        data.push_str(", world");
        LxJson(TestResponse { data })
    }
}
