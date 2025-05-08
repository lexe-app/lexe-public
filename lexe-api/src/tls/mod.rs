use std::sync::{Arc, LazyLock};

use asn1_rs::FromDer;
use rcgen::{DistinguishedName, DnType};
use rustls::{crypto::WebPkiSupportedAlgorithms, ClientConfig, ServerConfig};
use x509_parser::{
    certificate::X509Certificate, extensions::GeneralName, time::ASN1Time,
};

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// Certs and utilities related to Lexe's CA.
pub mod lexe_ca;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;
/// TLS newtypes, namely DER-encoded certs and cert keys.
pub mod types;

/// Allow accessing [`rustls`] via `lexe_api::tls`
pub use rustls;

use self::types::EdRcgenKeypair;

/// Whether the given DER-encoded cert is bound to the given DNS name.
/// Returns [`false`] if the cert failed to parse or is otherwise invalid.
#[must_use]
pub fn cert_contains_dns(cert_der: &[u8], expected_dns: &str) -> bool {
    fn contains_dns(cert_der: &[u8], expected_dns: &str) -> Option<()> {
        let (_unparsed, cert) = X509Certificate::from_der(cert_der).ok()?;

        let contains_dns = cert
            .subject_alternative_name()
            .ok()??
            .value
            .general_names
            .iter()
            .any(|gen_name| {
                matches!(
                    gen_name,
                    GeneralName::DNSName(dns) if *dns == expected_dns
                )
            });

        if contains_dns {
            Some(())
        } else {
            None
        }
    }

    contains_dns(cert_der, expected_dns).is_some()
}

/// Whether the given DER-encoded cert is currently valid and will be valid for
/// at least `buffer_days` more days. `buffer_days=0` can be used if you only
/// wish to check whether the cert is currently valid. Does not validate
/// anything other than expiry. Returns [`false`] if the cert failed to parse.
#[must_use]
pub fn cert_is_valid_for_at_least(cert_der: &[u8], buffer_days: u16) -> bool {
    fn is_valid_for_at_least(cert_der: &[u8], buffer_days: i64) -> Option<()> {
        use std::ops::Add;

        let (_unparsed, cert) = X509Certificate::from_der(cert_der).ok()?;

        let now = ASN1Time::now();
        let validity = cert.validity();

        if now < validity.not_before {
            return None;
        }
        if now > validity.not_after {
            return None;
        }

        let buffer_days_dur = time::Duration::days(buffer_days);

        // Check the same conditions `buffer_days` later.
        let now_plus_buffer = now.add(buffer_days_dur)?;
        if now_plus_buffer < validity.not_before {
            return None;
        }
        if now_plus_buffer > validity.not_after {
            return None;
        }

        Some(())
    }

    is_valid_for_at_least(cert_der, i64::from(buffer_days)).is_some()
}

/// Mozilla's webpki roots as a lazily-initialized [`rustls::RootCertStore`].
///
/// In some places where we must trust Mozilla's webpki roots, we add the trust
/// anchors manually to avoid enabling reqwest's `rustls-tls-webpki-roots`
/// feature, which propagates to other crates via feature unification.
///
/// It's safer to add the Mozilla roots manually than to have to remember to set
/// `.tls_built_in_root_certs(false)` in every [`reqwest`] client builder.
///
/// # Example
///
/// ```
/// # use std::time::Duration;
/// # use anyhow::Context;
/// use lexe_api::tls;
///
/// fn build_reqwest_client() -> anyhow::Result<reqwest::Client> {
///     let tls_config = tls::client_config_builder()
///         .with_root_certificates(tls::WEBPKI_ROOT_CERTS.clone())
///         .with_no_client_auth();
///
///     let client = reqwest::ClientBuilder::new()
///         .https_only(true)
///         .use_preconfigured_tls(tls_config)
///         .timeout(Duration::from_secs(10))
///         .build()
///         .context("reqwest::ClientBuilder::build failed")?;
///
///     Ok(client)
/// }
/// ```
pub static WEBPKI_ROOT_CERTS: LazyLock<Arc<rustls::RootCertStore>> =
    LazyLock::new(|| {
        let roots = webpki_roots::TLS_SERVER_ROOTS.to_vec();
        Arc::new(rustls::RootCertStore { roots })
    });

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
pub static LEXE_TLS_PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] =
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
///   [`rcgen::CertificateParams`] are considered. See struct for details.
/// - Any special fields or overrides can be specified using the `overrides`
///   closure. See usages for examples.
/// - `key_pair` and `alg` cannot be overridden.
pub fn build_rcgen_cert(
    common_name: &str,
    not_before: time::OffsetDateTime,
    not_after: time::OffsetDateTime,
    subject_alt_names: Vec<rcgen::SanType>,
    key_pair: EdRcgenKeypair,
    overrides: impl FnOnce(&mut rcgen::CertificateParams),
) -> rcgen::Certificate {
    let mut params = rcgen::CertificateParams::default();

    // alg: (can't be overridden)
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
    // key_pair: (can't be overridden)
    // use_authority_key_identifier_extension: false,
    // key_identifier_method: (can't be overridden)

    overrides(&mut params);

    // Prevent alg and keypair from being overridden to make panics impossible
    params.alg = &rcgen::PKCS_ED25519;
    params.key_pair = Some(key_pair.into_inner());

    // Use consistent method for deriving key identifiers
    params.key_identifier_method = rcgen::KeyIdMethod::Sha256;

    rcgen::Certificate::from_params(params)
        .expect("Can only panic if keypair doesn't match algorithm")
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
    use common::{api::error::BackendApiError, net, notify_once::NotifyOnce};
    use rustls::pki_types::ServerName;
    use serde::{Deserialize, Serialize};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tracing::info_span;

    use super::*;
    use crate::{
        rest::RestClient,
        server::{self, LayerConfig, LxJson},
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
        let shutdown = NotifyOnce::new();
        let tls_and_dns = Some((server_config, server_dns));
        const TEST_SPAN_NAME: &str = "(test-server)";
        let (server_task, server_url) = server::spawn_server_task(
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
