//! mTLS based on a shared [`RootSeed`]. We'll call this "shared seed" mTLS.
//!
//! ## Overview
//!
//! The client and server need to mutually authenticate each other and build a
//! secure channel via mTLS (mutual-auth TLS):
//!
//! - The app (client) wants to verify that it's talking to one of its
//!   previously provisioned nodes (server), since only a provisioned
//!   measurement could have access to the root seed.
//! - Likewise, a running node (server) has access to the [`RootSeed`], and
//!   wants to ensure that inbound connections (which may issue commands to
//!   spend money, withdraw funds etc) are coming from the node's actual owner
//!   (client). It will use the fact that only the node owner should have access
//!   to the [`RootSeed`].
//!
//! Under this "shared seed" mTLS scheme, both the client and server use their
//! root seed to independently derive a shared CA keypair, which they use to
//! sign their respective client and server end-entity certs which they present
//! to their counterparty. When authenticating their counterparty, they will
//! simply check that the presented end-entity cert has been signed by the
//! shared CA, which could only have been possible if the counterparty was also
//! able to derive the shared CA keypair.
//!
//! [`RootSeed`]: crate::root_seed::RootSeed

use std::sync::Arc;

use anyhow::Context;
use rustls::{
    client::{
        danger::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
        },
        WebPkiServerVerifier,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::WebPkiClientVerifier,
    DigitallySignedStruct, RootCertStore,
};

use super::lexe_ca;
#[cfg(doc)]
use crate::api::def::AppNodeRunApi;
use crate::{constants, env::DeployEnv, rng::Crng, root_seed::RootSeed};

/// TLS certs for shared [`RootSeed`]-based mTLS.
pub mod certs;

/// Server-side TLS config for [`AppNodeRunApi`].
/// Also returns the node's DNS name.
pub fn app_node_run_server_config(
    rng: &mut impl Crng,
    root_seed: &RootSeed,
) -> anyhow::Result<(rustls::ServerConfig, String)> {
    // Derive shared seed CA cert
    let ca_cert = certs::SharedSeedCaCert::from_root_seed(root_seed);
    let ca_cert_der = ca_cert
        .serialize_der_self_signed()
        .context("Failed to sign and serialize shared seed CA cert")?;

    // Build shared seed server cert and sign with derived CA
    let dns_name = constants::NODE_RUN_DNS.to_owned();
    let server_cert =
        certs::SharedSeedServerCert::from_rng(rng, dns_name.clone());
    let server_cert_der = server_cert
        .serialize_der_ca_signed(&ca_cert)
        .context("Failed to sign and serialize ephemeral server cert")?;
    let server_cert_key_der = server_cert.serialize_key_der();

    // Build our ClientCertVerifier which trusts our derived CA
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(ca_cert_der)
        .context("rustls failed to deserialize CA cert DER bytes")?;
    let client_cert_verifier = WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .context("Failed to build client cert verifier")?;

    let mut config = super::lexe_server_config()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(vec![server_cert_der], server_cert_key_der)
        .context("Failed to build rustls::ServerConfig")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok((config, dns_name))
}

/// Client-side TLS config for [`AppNodeRunApi`].
pub fn app_node_run_client_config(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    root_seed: &RootSeed,
) -> anyhow::Result<rustls::ClientConfig> {
    // Derive shared seed CA cert
    let ca_cert = certs::SharedSeedCaCert::from_root_seed(root_seed);

    // Build the client's server cert verifier:
    // - Shared seed verifier trusts the derived CA
    // - Public Lexe verifier trusts the hard-coded Lexe cert.
    let shared_seed_verifier = shared_seed_verifier(&ca_cert)
        .context("Failed to build shared seed verifier")?;
    let public_lexe_verifier = lexe_ca::lexe_server_verifier(deploy_env);
    let server_cert_verifier = AppNodeRunVerifier {
        shared_seed_verifier,
        public_lexe_verifier,
    };

    // Generate shared seed client cert and sign with derived CA
    let client_cert = certs::SharedSeedClientCert::generate_from_rng(rng);
    let client_cert_der = client_cert
        .serialize_der_ca_signed(&ca_cert)
        .context("Failed to sign and serialize ephemeral client cert")?;
    let client_cert_key_der = client_cert.serialize_key_der();

    let mut config = super::lexe_client_config()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        // NOTE: .with_single_cert() uses a client cert resolver which always
        // presents our client cert when asked. Does this introduce overhead by
        // needlessly presenting our shared seed client cert to the proxy which
        // doesn't actually require client auth? The answer is no, because the
        // proxy would then not send a CertificateRequest message during the
        // handshake, which is what actually prompts the client to send its
        // client cert. So the client never sends its cert to the proxy at all,
        // and defaults to a regular TLS handshake, with no mTLS overhead.
        // A custom client cert resolver is only needed if the proxy *also*
        // requires client auth, meaning we'd need to choose the correct cert to
        // present depending on whether the end entity is the proxy or the node.
        .with_client_auth_cert(vec![client_cert_der], client_cert_key_der)
        .context("Failed to build rustls::ClientConfig")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok(config)
}

/// Shorthand to build a [`ServerCertVerifier`] which trusts the derived CA.
pub fn shared_seed_verifier(
    ca_cert: &certs::SharedSeedCaCert,
) -> anyhow::Result<Arc<WebPkiServerVerifier>> {
    let ca_cert_der = ca_cert
        .serialize_der_self_signed()
        .context("Failed to sign and serialize shared seed CA cert")?;
    let mut roots = RootCertStore::empty();
    roots
        .add(ca_cert_der)
        .context("Failed to re-parse shared seed CA cert")?;
    let verifier = WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .context("Could not build shared seed server verifier")?;
    Ok(verifier)
}

/// The client's [`ServerCertVerifier`] for [`AppNodeRunApi`] TLS.
///
/// - When the app wishes to connect to a running node, it will make a request
///   to the node using a fake run DNS [`constants::NODE_RUN_DNS`]. However,
///   requests are first routed through lexe's reverse proxy, which parses the
///   fake run DNS in the SNI extension to determine whether we want to connect
///   to a running or provisioning node so it can route accordingly.
/// - The [`ServerName`] is given by the [`NodeClient`] reqwest client. This is
///   the gateway DNS when connecting to Lexe's proxy, otherwise it is the
///   node's fake run DNS. See [`NodeClient`]'s `run_url` for context.
/// - The [`AppNodeRunVerifier`] thus chooses between two "sub-verifiers"
///   according to the [`ServerName`] given to us by [`reqwest`]. We use the
///   public Lexe WebPKI verifier when establishing the outer TLS connection
///   with the gateway, and we use the shared seed verifier for the inner TLS
///   connection which terminates inside the user node SGX enclave.
///
/// [`NodeClient`]: crate::client::NodeClient
#[derive(Debug)]
struct AppNodeRunVerifier {
    /// `run.lexe.app` shared seed verifier - trusts the derived CA
    shared_seed_verifier: Arc<WebPkiServerVerifier>,
    /// `<TODO>.lexe.app` Lexe reverse proxy verifier - trusts the Lexe CA
    public_lexe_verifier: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for AppNodeRunVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        server_name: &ServerName,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let maybe_dns_name = match server_name {
            ServerName::DnsName(dns) => Some(dns.as_ref()),
            _ => None,
        };

        match maybe_dns_name {
            // Verify using derived shared seed CA when node is running
            Some(constants::NODE_RUN_DNS) =>
                self.shared_seed_verifier.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    now,
                ),
            // Other domains (i.e., node reverse proxy) verify using pinned
            // lexe CA
            // TODO(phlip9): this should be a strict DNS name, like
            // `proxy.lexe.app`. Come back once DNS names are more solid.
            _ => self.public_lexe_verifier.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            ),
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // We intentionally do not support TLSv1.2.
        let error = rustls::PeerIncompatible::ServerDoesNotSupportTls12Or13;
        Err(rustls::Error::PeerIncompatible(error))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &super::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        super::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
    }
}

#[cfg(test)]
mod test {
    use secrecy::Secret;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::rng::WeakRng;

    // TODO(max): Add a negative test: different root seeds should fail auth
    // (both client and server reject)
    // TODO(max): Add a snapshot test for deterministic derivation of shared
    // seed CA cert

    // test shared seed TLS handshake directly w/o any other axum/reqwest infra
    #[tokio::test]
    async fn test_tls_handshake_succeeds() {
        // a fake pair of connected streams
        let (client_stream, server_stream) = tokio::io::duplex(4096);

        let seed = [0x42; 32];

        // client tries to connect
        let client = async move {
            // should be able to independently derive CA key pair
            let seed = RootSeed::new(Secret::new(seed));
            let mut rng = WeakRng::from_u64(111);

            let deploy_env = DeployEnv::Dev;
            let config =
                app_node_run_client_config(&mut rng, deploy_env, &seed)
                    .unwrap();

            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let dns_name = constants::NODE_RUN_DNS;
            let sni = ServerName::try_from(dns_name).unwrap();
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

            let (config, _dns) =
                app_node_run_server_config(&mut rng, &seed).unwrap();
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
