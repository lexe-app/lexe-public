//! Lexe TLS configs, certs, and utilities.

use std::{str::FromStr, sync::LazyLock};

use asn1_rs::FromDer;
use byte_array::ByteArray;
use common::ed25519;
use rcgen::{DistinguishedName, DnType, string::Ia5String};
use x509_parser::{
    certificate::X509Certificate, extensions::GeneralName, time::ASN1Time,
};

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// ed25519 key pair extension trait (PEM ser/de).
pub mod ed25519_ext;
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
    LazyLock::new(|| {
        vec![rcgen::SanType::DnsName(
            Ia5String::from_str("lexe.app").unwrap(),
        )]
    });

/// Build an [`rcgen::CertificateParams`] with Lexe presets and optional
/// overrides.
/// - This builder function helps ensure that important fields in the inner
///   [`rcgen::CertificateParams`] are considered. See struct for details.
/// - Any special fields or overrides can be specified using the `overrides`
///   closure. See usages for examples.
/// - `key_pair` and `alg` cannot be overridden.
//
// TODO(phlip9): needs some normalizing with WebPKI
// - <https://letsencrypt.org/docs/profiles/>
// - <https://github.com/cabforum/servercert/blob/main/docs/BR.md>
// - use `ExplicitNoCa` for end-entity certs, use exact `KeyUsage` and
//   `ExtendedKeyUsage`, end-entities should just use first SAN as CN
pub fn build_rcgen_cert_params(
    common_name: &str,
    not_before: time::OffsetDateTime,
    not_after: time::OffsetDateTime,
    subject_alt_names: Vec<rcgen::SanType>,
    public_key: &ed25519::PublicKey,
    overrides: impl FnOnce(&mut rcgen::CertificateParams),
) -> rcgen::CertificateParams {
    let mut params = rcgen::CertificateParams::default();

    params.not_before = not_before;
    params.not_after = not_after;
    params.subject_alt_names = subject_alt_names;
    params.distinguished_name = lexe_distinguished_name(common_name);

    // is_ca: IsCa::NoCa,
    // key_usages: Vec::new(),
    // extended_key_usages: Vec::new(),
    // name_constraints: None,
    // crl_distribution_points: Vec::new(),
    // custom_extensions: Vec::new(),
    // use_authority_key_identifier_extension: false,

    // Custom caller overrides
    overrides(&mut params);

    // Preserve old `ring` pre-v0.14.0 behavior that uses
    // `key_identifier_method := Sha256(public-key)[0..20]` instead of
    // `key_identifier_method := Sha256(spki)[0..20]` used in newer `ring`.
    //
    // Conveniently also calculate the serial number at the same time, since
    // it's almost the same thing.
    //
    // RFC 5280 specifies at most 20 bytes for a serial/subject key identifier
    let pubkey_hash = {
        let hash = sha256::digest(public_key.as_slice());
        hash.as_slice()[0..20].to_vec()
    };

    // Only CA certs (including explicit self-signed-only certs) need a
    // `SubjectKeyIdentifier`.
    if matches!(params.is_ca, rcgen::IsCa::Ca(_) | rcgen::IsCa::ExplicitNoCa) {
        params.key_identifier_method =
            rcgen::KeyIdMethod::PreSpecified(pubkey_hash.clone());
    }

    // Use the (tweaked) pubkey hash as the cert serial number.
    let mut serial = pubkey_hash;
    serial[0] &= 0x7f; // MSB must be 0 to ensure encoding bignum in 20 B
    params.serial_number = Some(rcgen::SerialNumber::from(serial));

    params
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
