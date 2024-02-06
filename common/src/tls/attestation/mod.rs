//! (m)TLS based on SGX remote attestation.

use std::{sync::Arc, time::SystemTime};

use anyhow::Context;
use rustls::client::WebPkiVerifier;

use super::attestation;
use crate::{constants, ed25519, rng::Crng};

/// Self-signed x509 cert containing enclave remote attestation endorsements.
pub mod cert;
/// Get a quote for the running node enclave.
pub mod quote;
/// Verify remote attestation endorsements directly or embedded in x509 certs.
pub mod verifier;

pub fn node_provision_tls_config<R: Crng>(
    rng: &mut R,
    dns_name: String,
) -> anyhow::Result<rustls::ServerConfig> {
    // Generate a fresh key pair, which we'll use for the provisioning cert.
    let cert_key_pair = ed25519::KeyPair::from_rng(rng).to_rcgen();

    // Get our enclave measurement and cert pk quoted by the enclave
    // platform. This process binds the cert pk to the quote evidence. When
    // a client verifies the Quote, they can also trust that the cert was
    // generated on a valid, genuine enclave. Once this trust is settled,
    // they can safely provision secrets onto the enclave via the newly
    // established secure TLS channel.
    //
    // Returns the quote as an x509 cert extension that we'll embed in our
    // self-signed provisioning cert.
    let cert_pk = ed25519::PublicKey::try_from(&cert_key_pair).unwrap();
    let attestation = attestation::quote::quote_enclave(rng, &cert_pk)
        .context("Failed to get node enclave quoted")?;

    // Generate a self-signed x509 cert with the remote attestation embedded.
    let dns_names = vec![dns_name];
    let cert =
        cert::AttestationCert::new(cert_key_pair, dns_names, attestation)
            .context("Failed to generate remote attestation cert")?;
    let cert_der = rustls::Certificate(
        cert.serialize_der_signed()
            .context("Failed to sign and serialize attestation cert")?,
    );
    let cert_key_der = rustls::PrivateKey(cert.serialize_key_der());

    let mut config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], cert_key_der)
        .context("Failed to build TLS config")?;
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

pub fn client_provision_tls_config(
    use_sgx: bool,
    lexe_ca_cert: &rustls::Certificate,
    enclave_policy: attestation::verifier::EnclavePolicy,
) -> anyhow::Result<rustls::ClientConfig> {
    let attest_verifier = attestation::verifier::ServerCertVerifier {
        expect_dummy_quote: !use_sgx,
        enclave_policy,
    };

    let server_cert_verifier = ProvisionCertVerifier {
        lexe_verifier: super::lexe_verifier(lexe_ca_cert)?,
        attest_verifier,
    };

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        .with_no_client_auth();
    // TODO(phlip9): ensure this matches the reqwest config
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

/// The client's [`rustls::client::ServerCertVerifier`] for verifying the TLS
/// certs of a provisioning node, including remote attestation.
struct ProvisionCertVerifier {
    /// "<mr_short>.provision.lexe.app" remote attestation verifier
    attest_verifier: attestation::verifier::ServerCertVerifier,
    /// other (e.g., lexe reverse proxy) lexe CA verifier
    lexe_verifier: WebPkiVerifier,
}

impl rustls::client::ServerCertVerifier for ProvisionCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::Certificate,
        intermediates: &[rustls::Certificate],
        server_name: &rustls::ServerName,
        scts: &mut dyn Iterator<Item = &[u8]>,
        ocsp_response: &[u8],
        now: SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        let maybe_dns_name = match server_name {
            rustls::ServerName::DnsName(dns) => Some(dns.as_ref()),
            _ => None,
        };

        match maybe_dns_name {
            // Verify remote attestation cert when provisioning node
            Some(dns_name)
                if dns_name.ends_with(constants::NODE_PROVISION_DNS_SUFFIX) =>
                self.attest_verifier.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    scts,
                    ocsp_response,
                    now,
                ),
            // Other domains (i.e., node reverse proxy) verify using pinned
            // lexe CA
            // TODO(phlip9): this should be a strict DNS name, like
            // `proxy.lexe.app`. Come back once DNS names are more solid.
            _ => self.lexe_verifier.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                scts,
                ocsp_response,
                now,
            ),
        }
    }

    fn request_scts(&self) -> bool {
        false
    }
}
