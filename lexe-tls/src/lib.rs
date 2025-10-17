//! Lexe TLS configs, certs, and utilities.

use std::sync::LazyLock;

use asn1_rs::FromDer;
use common::ed25519;
use rcgen::{DistinguishedName, DnType};
use x509_parser::{
    certificate::X509Certificate, extensions::GeneralName, time::ASN1Time,
};

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// Certs and utilities related to Lexe's CA.
pub mod lexe_ca;
/// ECDSA P-256 key pairs for webpki TLS certs.
pub mod p256;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;
/// TLS newtypes, namely DER-encoded certs and cert keys.
pub mod types;

/// Re-export all of `lexe_tls_core`.
pub use lexe_tls_core::*;

use self::types::EdRcgenKeypair;

/// Whether the given DER-encoded cert is bound to the given DNS names.
///
/// Returns [`false`] if the cert doesn't contain all the dns names, fails to
/// parse, or is otherwise invalid.
#[must_use]
pub fn cert_contains_dns(cert_der: &[u8], expected_dns: &[&str]) -> bool {
    fn contains_dns(cert_der: &[u8], expected_dns: &[&str]) -> Option<()> {
        if expected_dns.is_empty() {
            return Some(());
        }

        let (_unparsed, cert) = X509Certificate::from_der(cert_der).ok()?;

        let sans = &cert.subject_alternative_name().ok()??.value.general_names;

        expected_dns
            .iter()
            .all(|dns_name| sans.contains(&GeneralName::DNSName(dns_name)))
            .then_some(())
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

/// A safe default for [`rcgen::CertificateParams::subject_alt_names`] when
/// there isn't a specific value that makes sense. Used for client / CA certs.
pub static DEFAULT_SUBJECT_ALT_NAMES: LazyLock<Vec<rcgen::SanType>> =
    LazyLock::new(|| vec![rcgen::SanType::DnsName("lexe.app".to_owned())]);

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
    key_pair: &ed25519::KeyPair,
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
    params.key_pair = Some(EdRcgenKeypair::from_ed25519(key_pair).into_inner());

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
    use std::sync::Arc;

    use anyhow::Context;
    use rustls::{ClientConfig, ServerConfig, pki_types::ServerName};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Conducts a TLS handshake without any other [`reqwest`]/[`axum`] infra,
    /// over a fake pair of connected streams. Returns the client and server
    /// results instead of panicking so that negative cases can be tested too.
    pub async fn do_tls_handshake(
        client_config: Arc<ClientConfig>,
        server_config: Arc<ServerConfig>,
        // This is the DNS name that the *client* expects the server to have.
        expected_dns: &str,
    ) -> [Result<(), String>; 2] {
        // a fake pair of connected streams
        let (client_stream, server_stream) = tokio::io::duplex(4096);

        // client connects, sends "hello", receives "goodbye"
        let client = async move {
            let connector = tokio_rustls::TlsConnector::from(client_config);
            let sni = ServerName::try_from(expected_dns.to_owned()).unwrap();
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
}
