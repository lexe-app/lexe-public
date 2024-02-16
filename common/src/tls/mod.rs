use std::sync::LazyLock;

use rcgen::{DistinguishedName, DnType};

use crate::ed25519;

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// Certs and utilities related to Lexe's CA.
pub mod lexe_ca;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;

/// Convenience struct to pass around a DER-encoded cert with its private key,
/// like `rcgen::CertifiedKey`. Can be passed into [`rustls::ConfigBuilder`].
pub struct CertWithKey {
    pub cert_der: rustls::Certificate,
    pub key_der: rustls::PrivateKey,
}

/// Lexe TLS protocol version: TLSv1.3
pub static LEXE_TLS_PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] =
    &[&rustls::version::TLS13];
/// Lexe cipher suite: specifically `TLS13_AES_128_GCM_SHA256`
pub static LEXE_CIPHER_SUITES: &[rustls::SupportedCipherSuite] =
    &[rustls::cipher_suite::TLS13_AES_128_GCM_SHA256];
/// Lexe key exchange group: X25519
pub static LEXE_KEY_EXCHANGE_GROUPS: &[&rustls::SupportedKxGroup] =
    &[&rustls::kx_group::X25519];
/// Lexe default value for [`rustls::ClientConfig::alpn_protocols`] and
/// [`rustls::ServerConfig::alpn_protocols`]: HTTP/1.1 and HTTP/2
// TODO(phlip9): ensure this matches the reqwest config
pub static LEXE_ALPN_PROTOCOLS: LazyLock<Vec<Vec<u8>>> =
    LazyLock::new(|| vec!["h2".into(), "http/1.1".into()]);
/// A safe default for [`rcgen::CertificateParams::subject_alt_names`] when
/// there isn't a specific value that makes sense. Used for client / CA certs.
pub static DEFAULT_SUBJECT_ALT_NAMES: LazyLock<Vec<rcgen::SanType>> =
    LazyLock::new(|| vec![rcgen::SanType::DnsName("lexe.app".to_owned())]);

/// Helper to get a builder for a [`rustls::ClientConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn lexe_default_client_config(
) -> rustls::ConfigBuilder<rustls::ClientConfig, rustls::WantsVerifier> {
    rustls::ClientConfig::builder()
        .with_cipher_suites(LEXE_CIPHER_SUITES)
        .with_kx_groups(LEXE_KEY_EXCHANGE_GROUPS)
        .with_protocol_versions(LEXE_TLS_PROTOCOL_VERSIONS)
        .expect("Invalid protocol versions")
}

/// Helper to get a builder for a [`rustls::ServerConfig`] with Lexe's presets.
/// NOTE: Remember: Set `alpn_protocols` to [`LEXE_ALPN_PROTOCOLS`] afterwards!
pub fn lexe_default_server_config(
) -> rustls::ConfigBuilder<rustls::ServerConfig, rustls::WantsVerifier> {
    rustls::ServerConfig::builder()
        .with_cipher_suites(LEXE_CIPHER_SUITES)
        .with_kx_groups(LEXE_KEY_EXCHANGE_GROUPS)
        .with_protocol_versions(LEXE_TLS_PROTOCOL_VERSIONS)
        .expect("Invalid protocol versions")
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
