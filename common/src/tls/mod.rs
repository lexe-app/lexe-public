use std::sync::{Arc, LazyLock};

use rcgen::{DistinguishedName, DnType};
use rustls::{
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, PrivateKeyDer},
    ClientConfig, ServerConfig,
};

use crate::ed25519;

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// Certs and utilities related to Lexe's CA.
pub mod lexe_ca;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;

/// Allow accessing [`rustls`] via `common::tls`
pub use rustls;

/// Convenience struct to pass around a DER-encoded cert with its private key,
/// like `rcgen::CertifiedKey`. Can be passed into [`rustls::ConfigBuilder`].
pub struct CertWithKey {
    pub cert_der: CertificateDer<'static>,
    pub key_der: PrivateKeyDer<'static>,
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
// TODO(phlip9): ensure this matches the reqwest config
pub static LEXE_ALPN_PROTOCOLS: LazyLock<Vec<Vec<u8>>> =
    LazyLock::new(|| vec!["h2".into(), "http/1.1".into()]);
/// A safe default for [`rcgen::CertificateParams::subject_alt_names`] when
/// there isn't a specific value that makes sense. Used for client / CA certs.
pub static DEFAULT_SUBJECT_ALT_NAMES: LazyLock<Vec<rcgen::SanType>> =
    LazyLock::new(|| vec![rcgen::SanType::DnsName("lexe.app".to_owned())]);

/// Helper to get a builder for a [`ClientConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn lexe_client_config(
) -> rustls::ConfigBuilder<ClientConfig, rustls::WantsVerifier> {
    // We use the correct provider and TLS versions here
    #[allow(clippy::disallowed_methods)]
    ClientConfig::builder_with_provider(LEXE_CRYPTO_PROVIDER.clone())
        .with_protocol_versions(LEXE_TLS_PROTOCOL_VERSIONS)
        .expect("Checked in tests")
}

/// Helper to get a builder for a [`ServerConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn lexe_server_config(
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
