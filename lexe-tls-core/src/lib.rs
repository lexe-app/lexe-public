//! Dependencies-minimized core for Lexe TLS.

use std::sync::{Arc, LazyLock};

/// Allow accessing [`rustls`] via `lexe_tls::rustls`.
pub use rustls;
use rustls::{crypto::WebPkiSupportedAlgorithms, ClientConfig, ServerConfig};
/// Allow accessing [`webpki_roots`] via `lexe_tls::webpki_roots`.
#[cfg(feature = "webpki-roots")]
pub use webpki_roots;

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

/// Mozilla's webpki roots as a lazily-initialized [`rustls::RootCertStore`].
///
/// In some places where we must trust Mozilla's webpki roots, we add the trust
/// anchors manually to avoid enabling reqwest's `rustls-tls-webpki-roots`
/// feature, which propagates to other crates via feature unification.
///
/// It's safer to add the Mozilla roots manually than to have to remember to set
/// `.tls_built_in_root_certs(false)` in every `reqwest` client builder.
///
/// # Example
///
/// ```ignore
/// # use std::time::Duration;
/// # use anyhow::Context;
/// #
/// fn build_reqwest_client() -> anyhow::Result<reqwest::Client> {
///     let tls_config = lexe_tls::client_config_builder()
///         .with_root_certificates(lexe_tls::WEBPKI_ROOT_CERTS.clone())
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
#[cfg(feature = "webpki-roots")]
pub static WEBPKI_ROOT_CERTS: std::sync::LazyLock<
    std::sync::Arc<rustls::RootCertStore>,
> = LazyLock::new(|| {
    let roots = webpki_roots::TLS_SERVER_ROOTS.to_vec();
    Arc::new(rustls::RootCertStore { roots })
});
