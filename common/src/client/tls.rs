//! Build complete TLS configurations for clients to verify remote endpoints.

use std::{sync::Arc, time::SystemTime};

use anyhow::Context;
use rustls::{client::WebPkiVerifier, sign::CertifiedKey, RootCertStore};

use crate::{
    attest,
    attest::cert::AttestationCert,
    client::certs::{CaCert, ClientCert, NodeCert},
    constants, ed25519,
    rng::Crng,
    root_seed::RootSeed,
};

/// The client's [`rustls::client::ServerCertVerifier`] for verifiying a
/// remote server's TLS certs.
///
/// Currently we verify 3 different cases, depending on the remote cert's dns
/// name.
///
/// 1. "run.lexe.tech" => verify one of the client's previously provisioned
///    nodes.
/// 2. "provision.lexe.tech" => verify the remote attestation TLS cert for a
///    machine the client might want to provision with their secrets.
/// 3. other => verify a lexe endpoint, using a pinned CA cert.
struct ServerCertVerifier {
    /// "run.lexe.tech" node-cert verifier
    node_verifier: WebPkiVerifier,

    /// "provision.lexe.tech" remote attestation verifier
    attest_verifier: attest::ServerCertVerifier,

    /// other (e.g., lexe reverse proxy) lexe CA verifier
    lexe_verifier: WebPkiVerifier,
}

/// The client's mTLS cert resolver. During a TLS handshake, this resolver
/// decides whether to present the client's cert to the server.
///
/// Currently the client only provides their cert when connecting to their
/// running and already-provisioned node.
struct ClientCertResolver {
    /// Our client's serialized cert + cert key pair
    client_cert: Arc<CertifiedKey>,
}

// -- rustls TLS configs -- //

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
    let attestation = attest::quote_enclave(rng, &cert_pk)
        .context("Failed to get node enclave quoted")?;

    // Generate a self-signed x509 cert with the remote attestation embedded.
    let dns_names = vec![dns_name];
    let cert = AttestationCert::new(cert_key_pair, dns_names, attestation)
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

pub fn node_run_tls_config<R: Crng>(
    rng: &mut R,
    seed: &RootSeed,
    dns_names: Vec<String>,
) -> anyhow::Result<rustls::ServerConfig> {
    // derive the shared client-node CA cert from the root seed
    let ca_cert_key_pair = seed.derive_client_ca_key_pair();
    let ca_cert = CaCert::from_key_pair(ca_cert_key_pair)
        .context("Failed to build node-client CA cert")?;
    let ca_cert_der = rustls::Certificate(
        ca_cert
            .serialize_der_signed()
            .context("Failed to sign and serialize node-client CA cert")?,
    );

    // build node cert and sign w/ the CA cert
    let node_key_pair = ed25519::KeyPair::from_rng(rng).to_rcgen();
    let node_cert = NodeCert::from_key_pair(node_key_pair, dns_names)
        .context("Failed to build ephemeral node cert")?;
    let node_cert_der = rustls::Certificate(
        node_cert
            .serialize_der_signed(&ca_cert)
            .context("Failed to sign and serialize ephemeral client cert")?,
    );
    let node_key_der = rustls::PrivateKey(node_cert.serialize_key_der());

    // client cert trust root is just the derived CA cert
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(&ca_cert_der)
        .context("rustls failed to deserialize CA cert DER bytes")?;

    // subject alt names for client are not useful here; just check for valid
    // cert chain
    let client_verifier =
        rustls::server::AllowAnyAuthenticatedClient::new(roots);

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(vec![node_cert_der], node_key_der)
        .context("Failed to build rustls::ServerConfig")?;
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

pub fn client_tls_config<R: Crng>(
    rng: &mut R,
    lexe_trust_anchor: &rustls::Certificate,
    seed: &RootSeed,
    attest_verifier: attest::ServerCertVerifier,
) -> anyhow::Result<rustls::ClientConfig> {
    // derive the shared client-node CA cert from the root seed
    let ca_cert_key_pair = seed.derive_client_ca_key_pair();
    let ca_cert = CaCert::from_key_pair(ca_cert_key_pair)
        .context("Failed to build node-client CA cert")?;
    let ca_cert_der = rustls::Certificate(
        ca_cert
            .serialize_der_signed()
            .context("Failed to sign and serialize node-client CA cert")?,
    );

    // the derived CA cert is our trust root for node connections
    let mut node_roots = RootCertStore::empty();
    node_roots
        .add(&ca_cert_der)
        .context("Failed to re-parse node-client CA cert")?;
    let node_ct_policy = None;
    let node_verifier = WebPkiVerifier::new(node_roots, node_ct_policy);

    let server_cert_verifier = ServerCertVerifier {
        lexe_verifier: lexe_verifier(lexe_trust_anchor)?,
        node_verifier,
        attest_verifier,
    };

    let client_cert_resolver = ClientCertResolver::new(rng, &ca_cert)
        .context("Failed to build client certs")?;

    // TODO(phlip9): use exactly TLSv1.3, ciphersuite TLS13_AES_128_GCM_SHA256,
    // and key exchange X25519
    let mut config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        .with_client_cert_resolver(Arc::new(client_cert_resolver));
    // TODO(phlip9): ensure this matches the reqwest config
    config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    Ok(config)
}

fn lexe_verifier(
    lexe_trust_anchor: &rustls::Certificate,
) -> anyhow::Result<WebPkiVerifier> {
    let mut lexe_roots = RootCertStore::empty();
    lexe_roots
        .add(lexe_trust_anchor)
        .context("Failed to deserialize lexe trust anchor")?;
    // TODO(phlip9): our web-facing certs will actually support cert
    // transparency
    let lexe_ct_policy = None;
    let lexe_verifier = WebPkiVerifier::new(lexe_roots, lexe_ct_policy);
    Ok(lexe_verifier)
}

// TODO(phlip9): need to replace this when we get the Lexe CA certs wired
// through
pub fn dummy_lexe_ca_cert() -> rustls::Certificate {
    let dns_names = vec!["localhost".to_owned()];
    let cert_params = rcgen::CertificateParams::new(dns_names);
    let fake_lexe_cert = rcgen::Certificate::from_params(cert_params).unwrap();
    rustls::Certificate(fake_lexe_cert.serialize_der().unwrap())
}

// -- impl ServerCertVerifier -- //

impl rustls::client::ServerCertVerifier for ServerCertVerifier {
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
            // Verify using derived node-client CA when node is running
            Some(constants::NODE_RUN_DNS) =>
                self.node_verifier.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    scts,
                    ocsp_response,
                    now,
                ),
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
            // `proxy.lexe.tech`. Come back once DNS names are more solid.
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

impl ClientCertResolver {
    /// Samples a new child cert then signs with the node-client CA.
    fn new<R: Crng>(rng: &mut R, ca_cert: &CaCert) -> anyhow::Result<Self> {
        // sample an ephemeral key pair for the child cert
        let client_cert_key_pair = ed25519::KeyPair::from_rng(rng).to_rcgen();
        let client_cert = ClientCert::from_key_pair(client_cert_key_pair)
            .context("Failed to build ephemeral client cert")?;
        let client_cert_der = rustls::Certificate(
            client_cert.serialize_der_signed(ca_cert).context(
                "Failed to sign and serialize ephemeral client cert",
            )?,
        );
        let client_key_der =
            rustls::PrivateKey(client_cert.serialize_key_der());

        // massage into rustls types
        let signing_key = rustls::sign::any_eddsa_type(&client_key_der)
            .context("Failed to parse client cert ed25519 sk")?;
        let certified_key =
            rustls::sign::CertifiedKey::new(vec![client_cert_der], signing_key);

        Ok(Self {
            client_cert: Arc::new(certified_key),
        })
    }
}

impl rustls::client::ResolvesClientCert for ClientCertResolver {
    fn resolve(
        &self,
        // These are undecoded and unverified by the rustls library, but should
        // be expected to contain DER encodinged X501 NAMEs.
        acceptable_issuers: &[&[u8]],
        // The server's supported signature schemes.
        _sigschemes: &[rustls::SignatureScheme],
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        // TODO(phlip9): actually check against server's advertised sigschemes.
        // right now both sides only support ed25519, so this doesn't really
        // matter.

        let remote_wants_client_cert = acceptable_issuers
            .iter()
            .any(|der| CaCert::is_matching_issuer_der(der));

        if remote_wants_client_cert {
            Some(self.client_cert.clone())
        } else {
            None
        }
    }

    fn has_certs(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use secrecy::Secret;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls;

    use super::*;
    use crate::{attest::EnclavePolicy, rng::WeakRng};

    // test node-client TLS handshake directly w/o any other warp/reqwest infra
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
            let mut rng = WeakRng::from_u64(111);

            // should be unused since no proxy
            let lexe_root_key_pair =
                ed25519::KeyPair::from_seed(&[0xa1; 32]).to_rcgen();
            let lexe_root = CaCert::from_key_pair(lexe_root_key_pair)
                .unwrap()
                .serialize_der_signed()
                .unwrap();
            let lexe_root = rustls::Certificate(lexe_root);

            let attest_verifier = attest::ServerCertVerifier {
                expect_dummy_quote: true,
                enclave_policy: EnclavePolicy::dangerous_trust_any(),
            };

            let config =
                client_tls_config(&mut rng, &lexe_root, &seed, attest_verifier)
                    .unwrap();

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
            let mut rng = WeakRng::from_u64(222);

            let dns_names = vec![dns_name.to_owned()];
            let config =
                node_run_tls_config(&mut rng, &seed, dns_names).unwrap();
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
