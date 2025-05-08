//! mTLS based on a shared [`RootSeed`]. We'll call this "shared seed" mTLS.
//!
//! ## Overview
//!
//! Security is based on the fact that the [`RootSeed`] is only known to the
//! user and a previously provisioned node.
//!
//! There are two forms:
//!
//! 1) "Ephemeral": Client and server both have a copy of the [`RootSeed`]. Both
//!    use the [`RootSeed`] to independently and deterministically derive a
//!    non-expiring "ephemeral cert issuing" CA which then issues a short-lived
//!    client or server end-entity cert which they present to the other side.
//!    The client and server verify that the other side has presented a cert
//!    signed by the derived CA.
//!
//! 2) "Revocable": Node (server) deterministically derives a "revocable cert
//!    issuing" CA. The app requests the node to issue an "revocable" client
//!    cert. The issued client cert does not encode an expiration. Instead, its
//!    expiration is managed at the application level via a cert whitelist which
//!    tracks each client cert's serial number and expiration. These client
//!    certs can be given to SDK clients and revoked at any time.
//!
//! ## Client and server cert verification
//!
//! - Client: Trusts only the [`EphemeralIssuingCaCert`], since the node always
//!   has access to the root seed and can therefore rederive the CA cert and
//!   issue itself a fresh server cert.
//!
//! - Server: Trusts *either* the [`EphemeralIssuingCaCert`] or the
//!   [`RevocableIssuingCaCert`], but with some differences:
//!   - All certs signed by the [`EphemeralIssuingCaCert`] are automatically
//!     trusted.
//!   - Certs signed by the [`RevocableIssuingCaCert`] must additionally appear
//!     in the client cert whitelist, must not be expired, and must and not be
//!     revoked.
//!
//! ## Which certs / secrets are stored where?
//!
//! - Node: Holds [`RootSeed`], derives ephemeral CA cert and eph server cert.
//! - App: Holds [`RootSeed`], derives ephemeral CA cert and eph client cert.
//! - SDK: Holds:
//!   - [`RevocableIssuingCaCert`] (DER only, no keypair)
//!   - [`RevocableClientCert`] (with keypair)
//!
//! ## Certificate hierarchy
//!
//! [`RootSeed`]
//! |
//! |___ [`EphemeralIssuingCaCert`]: Deterministically derived, expires never
//! |   |
//! |   |___ [`EphemeralClientCert`]: Expires in 90 days
//! |   |
//! |   |___ [`EphemeralServerCert`]: Expires in 90 days
//! |
//! |___ [`RevocableIssuingCaCert`]: Deterministically derived, expires never
//!     |
//!     |___ [`RevocableClientCert`]: Issued by parent; expires never.
//!     |
//!     |___ (no server cert; node always presents [`EphemeralServerCert`])
//!
//! [`RootSeed`]: common::root_seed::RootSeed
//! [`EphemeralIssuingCaCert`]: crate::tls::shared_seed::certs::EphemeralIssuingCaCert
//! [`EphemeralClientCert`]: crate::tls::shared_seed::certs::EphemeralClientCert
//! [`EphemeralServerCert`]: crate::tls::shared_seed::certs::EphemeralServerCert
//! [`RevocableIssuingCaCert`]: crate::tls::shared_seed::certs::RevocableIssuingCaCert
//! [`RevocableClientCert`]: crate::tls::shared_seed::certs::RevocableClientCert

// TODO(max): Only the app (not an SDK) should have the power to call the "issue
// new cert" endpoint.

use std::sync::Arc;

use anyhow::Context;
use common::{constants, env::DeployEnv, rng::Crng, root_seed::RootSeed};
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

/// TLS certs for shared [`RootSeed`]-based mTLS.
pub mod certs;

/// Server-side TLS config for `AppNodeRunApi`.
/// Also returns the node's DNS name.
pub fn app_node_run_server_config(
    rng: &mut impl Crng,
    root_seed: &RootSeed,
) -> anyhow::Result<(rustls::ServerConfig, String)> {
    // Derive ephemeral issuing CA cert
    let ca_cert = certs::EphemeralIssuingCaCert::from_root_seed(root_seed);
    let ca_cert_der = ca_cert
        .serialize_der_self_signed()
        .context("Failed to sign and serialize ephemeral CA cert")?;

    // Build ephemeral server cert and sign with derived CA
    let dns_name = constants::NODE_RUN_DNS.to_owned();
    let server_cert =
        certs::EphemeralServerCert::from_rng(rng, dns_name.clone());
    let server_cert_der = server_cert
        .serialize_der_ca_signed(&ca_cert)
        .context("Failed to sign and serialize ephemeral server cert")?;
    let server_cert_key_der = server_cert.serialize_key_der();

    // Build our ClientCertVerifier which trusts our derived CA
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(ca_cert_der.into())
        .context("rustls failed to deserialize CA cert DER bytes")?;
    let client_cert_verifier = WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .context("Failed to build client cert verifier")?;

    let mut config = super::server_config_builder()
        .with_client_cert_verifier(client_cert_verifier)
        .with_single_cert(
            vec![server_cert_der.into()],
            server_cert_key_der.into(),
        )
        .context("Failed to build rustls::ServerConfig")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok((config, dns_name))
}

/// Client-side TLS config for `AppNodeRunApi`.
pub fn app_node_run_client_config(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    root_seed: &RootSeed,
) -> anyhow::Result<rustls::ClientConfig> {
    // Derive ephemeral issuing CA cert
    let ca_cert = certs::EphemeralIssuingCaCert::from_root_seed(root_seed);

    // Build the client's server cert verifier:
    // - Ephemeral CA verifier trusts the ephemeral issuing CA
    // - Public Lexe verifier trusts the hard-coded Lexe cert.
    let ephemeral_ca_verifier = ephemeral_ca_verifier(&ca_cert)
        .context("Failed to build ephemeral CA verifier")?;
    let lexe_server_verifier = lexe_ca::lexe_server_verifier(deploy_env);
    let server_cert_verifier = AppNodeRunVerifier {
        ephemeral_ca_verifier,
        lexe_server_verifier,
    };

    // Generate ephemeral client cert and sign with derived CA
    let client_cert = certs::EphemeralClientCert::generate_from_rng(rng);
    let client_cert_der = client_cert
        .serialize_der_ca_signed(&ca_cert)
        .context("Failed to sign and serialize ephemeral client cert")?;
    let client_cert_key_der = client_cert.serialize_key_der();

    let mut config = super::client_config_builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        // NOTE: .with_single_cert() uses a client cert resolver which always
        // presents our client cert when asked. Does this introduce overhead by
        // needlessly presenting our ephemeral client cert to the proxy which
        // doesn't actually require client auth? The answer is no, because the
        // proxy would then not send a CertificateRequest message during the
        // handshake, which is what actually prompts the client to send its
        // client cert. So the client never sends its cert to the proxy at all,
        // and defaults to a regular TLS handshake, with no mTLS overhead.
        // A custom client cert resolver is only needed if the proxy *also*
        // requires client auth, meaning we'd need to choose the correct cert to
        // present depending on whether the end entity is the proxy or the node.
        .with_client_auth_cert(
            vec![client_cert_der.into()],
            client_cert_key_der.into(),
        )
        .context("Failed to build rustls::ClientConfig")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok(config)
}

/// Build a [`ServerCertVerifier`] which trusts the "ephemeral issuing" CA.
pub fn ephemeral_ca_verifier(
    ca_cert: &certs::EphemeralIssuingCaCert,
) -> anyhow::Result<Arc<WebPkiServerVerifier>> {
    let ca_cert_der = ca_cert
        .serialize_der_self_signed()
        .context("Failed to sign and serialize ephemeral CA cert")?;
    let mut roots = RootCertStore::empty();
    roots
        .add(ca_cert_der.into())
        .context("Failed to re-parse ephemeral CA cert")?;
    let verifier = WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .context("Could not build ephemeral server verifier")?;
    Ok(verifier)
}

/// The client's [`ServerCertVerifier`] for `AppNodeRunApi` TLS.
///
/// - When the app wishes to connect to a running node, it will make a request
///   to the node using a fake run DNS [`constants::NODE_RUN_DNS`]. However,
///   requests are first routed through lexe's reverse proxy, which parses the
///   fake run DNS in the SNI extension to determine whether we want to connect
///   to a running or provisioning node so it can route accordingly.
/// - The [`ServerName`] is given by the `NodeClient` reqwest client. This is
///   the gateway DNS when connecting to Lexe's proxy, otherwise it is the
///   node's fake run DNS. See `NodeClient`'s `run_url` for context.
/// - The [`AppNodeRunVerifier`] thus chooses between two "sub-verifiers"
///   according to the [`ServerName`] given to us by [`reqwest`]. We use the
///   public Lexe WebPKI verifier when establishing the outer TLS connection
///   with the gateway, and we use the ephemeral CA verifier for the inner TLS
///   connection which terminates inside the user node SGX enclave.
#[derive(Debug)]
struct AppNodeRunVerifier {
    /// `run.lexe.app` verifier - trusts the "ephemeral issuing" CA
    ephemeral_ca_verifier: Arc<WebPkiServerVerifier>,
    /// Lexe server verifier - trusts the Lexe CA
    lexe_server_verifier: Arc<WebPkiServerVerifier>,
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
            // Verify using ephemeral issuing CA when node is running
            Some(constants::NODE_RUN_DNS) =>
                self.ephemeral_ca_verifier.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    now,
                ),
            // Other domains (i.e., node reverse proxy) verify using lexe CA
            _ => self.lexe_server_verifier.verify_server_cert(
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
    use std::sync::Arc;

    use common::{env::DeployEnv, rng::FastRng, root_seed::RootSeed};
    use secrecy::Secret;

    use super::*;
    use crate::tls::test_utils;

    /// App->Node TLS handshake should succeed when using the same seed.
    #[tokio::test]
    async fn app_node_run_handshake_succeeds() {
        let client_seed = RootSeed::new(Secret::new([0x42; 32]));
        let server_seed = RootSeed::new(Secret::new([0x42; 32]));

        let [client_result, server_result] =
            do_app_node_run_tls_handshake(&client_seed, &server_seed).await;

        client_result.unwrap();
        server_result.unwrap();
    }

    /// App->Node TLS handshake should fail when using different seeds.
    #[tokio::test]
    async fn app_node_run_handshake_fails_with_different_seeds() {
        let client_seed = RootSeed::new(Secret::new([0x42; 32]));
        let server_seed = RootSeed::new(Secret::new([0x69; 32]));

        let [client_result, server_result] =
            do_app_node_run_tls_handshake(&client_seed, &server_seed).await;

        assert!(client_result.unwrap_err().contains("Client didn't connect"));
        assert!(server_result.unwrap_err().contains("Server didn't accept"));
    }

    // Shorthand to do a App->Node Run TLS handshake.
    async fn do_app_node_run_tls_handshake(
        client_seed: &RootSeed,
        server_seed: &RootSeed,
    ) -> [Result<(), String>; 2] {
        let mut rng = FastRng::from_u64(20240514);
        let deploy_env = DeployEnv::Dev;

        let client_config =
            app_node_run_client_config(&mut rng, deploy_env, client_seed)
                .map(Arc::new)
                .unwrap();
        let (server_config, server_dns) =
            app_node_run_server_config(&mut rng, server_seed)
                .map(|(c, d)| (Arc::new(c), d))
                .unwrap();

        test_utils::do_tls_handshake(client_config, server_config, server_dns)
            .await
    }
}
