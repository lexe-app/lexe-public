//! TODO

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use rustls::client::{ServerCertVerifier, WebPkiVerifier};
use rustls::RootCertStore;

use crate::client::certs::{CaCert, ClientCert, NodeCert};
use crate::rng::Crng;
use crate::root_seed::RootSeed;
use crate::{attest, ed25519};

/// A [`rustls`] TLS cert verifier for the client to connect to a now
/// provisioned and possibly running node through the reverse proxy.
struct ClientRunCertVerifier {
    lexe_verifier: WebPkiVerifier,
    node_verifier: WebPkiVerifier,
}

/// A [`rustls`] TLS cert verifier for the client to connect to a provisioning
/// node through the reverse proxy.
struct ClientProvisionCertVerifier {
    lexe_verifier: WebPkiVerifier,
    attest_verifier: attest::ServerCertVerifier,
}

// -- rustls TLS configs -- //

pub fn node_tls_config(
    node_cert: &NodeCert,
    ca_cert: &CaCert,
) -> anyhow::Result<rustls::ServerConfig> {
    let ca_cert_der = ca_cert
        .serialize_der_signed()
        .context("Failed to self-sign + DER-serialize CA cert")?;
    let node_cert_der = node_cert
        .serialize_der_signed(ca_cert)
        .context("Failed to sign + DER-serialize node cert w/ CA cert")?;
    let node_key_der = node_cert.serialize_key_der();

    let mut trust_anchors = rustls::RootCertStore::empty();
    trust_anchors
        .add(&rustls::Certificate(ca_cert_der))
        .context("rustls failed to deserialize CA cert DER bytes")?;

    let client_verifier =
        rustls::server::AllowAnyAuthenticatedClient::new(trust_anchors);

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(
            vec![rustls::Certificate(node_cert_der)],
            rustls::PrivateKey(node_key_der),
        )
        .context("Failed to build rustls::ServerConfig")?;
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

pub fn client_provision_tls_config(
    lexe_trust_anchor: &rustls::Certificate,
    expect_dummy_quote: bool,
    enclave_policy: attest::EnclavePolicy,
) -> Result<rustls::ClientConfig> {
    let verifier = ClientProvisionCertVerifier {
        lexe_verifier: lexe_verifier(lexe_trust_anchor)?,
        attest_verifier: attest::ServerCertVerifier {
            expect_dummy_quote,
            enclave_policy,
        },
    };

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    // TODO(phlip9): ensure this matches the reqwest config
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

pub fn client_run_tls_config(
    rng: &mut dyn Crng,
    lexe_trust_anchor: &rustls::Certificate,
    root_seed: &RootSeed,
) -> Result<rustls::ClientConfig> {
    // derive the shared client-node CA cert from the root seed
    let ca_cert_key_pair = root_seed.derive_client_ca_key_pair();
    let ca_cert = CaCert::from_key_pair(ca_cert_key_pair)
        .context("Failed to build node-client CA cert")?;
    let ca_cert_der = rustls::Certificate(
        ca_cert
            .serialize_der_signed()
            .context("Failed to sign and serialize node-client CA cert")?,
    );

    // the derived CA cert is our trust root for node connections
    let mut roots = RootCertStore::empty();
    roots
        .add(&ca_cert_der)
        .context("Failed to re-parse node-client CA cert")?;
    let node_verifier = WebPkiVerifier::new(roots, None);

    let verifier = ClientRunCertVerifier {
        lexe_verifier: lexe_verifier(lexe_trust_anchor)?,
        node_verifier,
    };

    // sample an ephemeral key pair for the child cert
    let client_cert_key_pair = ed25519::gen_key_pair(rng);
    let client_cert = ClientCert::from_key_pair(client_cert_key_pair)
        .context("Failed to build ephemeral client cert")?;
    let client_cert_der = rustls::Certificate(
        client_cert
            .serialize_der_signed(&ca_cert)
            .context("Failed to sign and serialize ephemeral client cert")?,
    );
    let client_key_der = rustls::PrivateKey(client_cert.serialize_key_der());

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(verifier))
        // TODO(phlip9): do we need to use a custom cert resovler that doesn't
        // send the client cert for the reverse proxy connection?
        .with_single_cert(vec![client_cert_der], client_key_der)
        .context("Failed to build rustls::ClientConfig")?;
    // TODO(phlip9): ensure this matches the reqwest config
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

fn lexe_verifier(
    lexe_trust_anchor: &rustls::Certificate,
) -> Result<WebPkiVerifier> {
    let mut lexe_roots = RootCertStore::empty();
    lexe_roots
        .add(lexe_trust_anchor)
        .context("Failed to deserialize lexe trust anchor")?;
    // TODO(phlip9): our web-facing certs will actually support cert
    // transparency
    let ct_policy = None;
    let lexe_verifier = WebPkiVerifier::new(lexe_roots, ct_policy);
    Ok(lexe_verifier)
}

// -- impl ClientProvisionCertVerifier -- //

impl ServerCertVerifier for ClientProvisionCertVerifier {
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
            Some("provision.lexe.tech") => {
                self.attest_verifier.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    scts,
                    ocsp_response,
                    now,
                )
            }
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

// -- impl ClientProvisionCertVerifier -- //

impl ServerCertVerifier for ClientRunCertVerifier {
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
            Some("run.lexe.tech") => self.node_verifier.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                scts,
                ocsp_response,
                now,
            ),
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

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use secrecy::Secret;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls;

    use super::*;
    use crate::rng::SmallRng;

    #[tokio::test]
    async fn test_tls_handshake_succeeds() {
        // a fake pair of connected streams
        let (client_stream, server_stream) = duplex(4096);

        let seed = [0x42; 32];
        let dns_name = "run.lexe.tech";

        // client tries to connect
        let client = async move {
            // should be able to independently derive CA key pair
            let seed = RootSeed::new(Secret::new(seed));
            let mut rng = SmallRng::new();

            // should be unused since no proxy
            let lexe_root =
                CaCert::from_key_pair(ed25519::from_seed(&[0xa1; 32]))
                    .unwrap()
                    .serialize_der_signed()
                    .unwrap();
            let lexe_root = rustls::Certificate(lexe_root);

            let config =
                client_run_tls_config(&mut rng, &lexe_root, &seed).unwrap();

            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let sni = rustls::ServerName::try_from(dns_name).unwrap();
            let mut stream =
                connector.connect(sni, client_stream).await.unwrap();

            // client: >> send "hello"

            stream.write_all(b"hello").await.unwrap();
            stream.flush().await.unwrap();
            stream.shutdown().await.unwrap();

            // client: << recv "goodbye"

            let mut resp = Vec::new();
            stream.read_to_end(&mut resp).await.unwrap();

            assert_eq!(&resp, b"goodbye");
        };

        // node should accept handshake
        let node = async move {
            // should be able to independently derive CA key pair
            let seed = RootSeed::new(Secret::new(seed));
            let ca_key_pair = seed.derive_client_ca_key_pair();
            let ca_cert = CaCert::from_key_pair(ca_key_pair).unwrap();

            let node_key_pair = ed25519::from_seed(&[0xf0; 32]);
            let dns_names = vec![dns_name.to_owned()];
            let node_cert =
                NodeCert::from_key_pair(node_key_pair, dns_names).unwrap();

            let config = node_tls_config(&node_cert, &ca_cert).unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let mut stream = acceptor.accept(server_stream).await.unwrap();

            // node: >> recv "hello"

            let mut req = Vec::new();
            stream.read_to_end(&mut req).await.unwrap();

            assert_eq!(&req, b"hello");

            // node: << send "goodbye"

            stream.write_all(b"goodbye").await.unwrap();
            stream.shutdown().await.unwrap();
        };

        tokio::join!(client, node);
    }
}
