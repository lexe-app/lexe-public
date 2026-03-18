//! # mTLS using SGX remote attestation
//!
//! This crate contains the client-side SGX attestation and TLS cert
//! verification logic.
//!
//! See: `lexe-tls-attest-server` for server/enclave SGX attestation and TLS
//! server cert generation logic.

use std::sync::Arc;

use lexe_common::{constants, env::DeployEnv};
use lexe_enclave_core::enclave::Measurement;
use rustls::{
    DigitallySignedStruct,
    client::{
        WebPkiServerVerifier,
        danger::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
        },
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
};

use crate::{attestation::verifier::EnclavePolicy, lexe_ca};

/// An x509 cert extension containing remote attestation endorsements.
pub mod cert;
/// Get a quote for the running node enclave.
pub mod quote;
/// Verify remote attestation endorsements directly or embedded in x509 certs.
pub mod verifier;

/// Client-side TLS config for `AppNodeProvisionApi`.
pub fn app_node_provision_client_config(
    use_sgx: bool,
    deploy_env: DeployEnv,
    measurement: Measurement,
) -> rustls::ClientConfig {
    let enclave_policy = EnclavePolicy::trust_measurements_with_signer(
        use_sgx,
        deploy_env,
        vec![measurement],
    );
    let attestation_verifier = verifier::AttestationCertVerifier {
        expect_dummy_quote: !use_sgx,
        enclave_policy,
    };
    let lexe_server_verifier = lexe_ca::lexe_server_verifier(deploy_env);

    let server_cert_verifier = AppNodeProvisionVerifier {
        lexe_server_verifier,
        attestation_verifier,
    };

    let mut config = lexe_tls_core::client_config_builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        .with_no_client_auth();
    config
        .alpn_protocols
        .clone_from(&lexe_tls_core::LEXE_ALPN_PROTOCOLS);

    config
}

/// The client's [`ServerCertVerifier`] for `AppNodeProvisionApi` TLS.
///
/// - When the app wishes to provision, it will make a request to the node using
///   a fake provision DNS given by [`constants::node_provision_dns`]. However,
///   requests are first routed through lexe's reverse proxy, which parses the
///   fake provision DNS in the SNI extension to determine (1) whether we want
///   to connect to a running or provisioning node and (2) the [`MrShort`] of
///   the measurement we wish to provision so it can route accordingly.
/// - The [`ServerName`] is given by the `NodeClient` reqwest client. This is
///   the gateway DNS when connecting to Lexe's proxy, otherwise it is the
///   node's fake provision DNS. See `NodeClient::provision` for details.
/// - The [`AppNodeProvisionVerifier`] thus chooses between two "sub-verifiers"
///   according to the [`ServerName`] given to us by `reqwest`. We use the
///   public Lexe WebPKI verifier when establishing the outer TLS connection
///   with the gateway, and we use the remote attestation verifier for the inner
///   TLS connection which terminates inside the user node SGX enclave.
///
/// [`MrShort`]: lexe_enclave_core::enclave::MrShort
#[derive(Debug)]
struct AppNodeProvisionVerifier {
    /// `<mr_short>.provision.lexe.app` remote attestation verifier
    attestation_verifier: verifier::AttestationCertVerifier,
    /// Lexe server verifier - trusts the Lexe CA
    lexe_server_verifier: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for AppNodeProvisionVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        // This comes from the reqwest client, not the server.
        server_name: &ServerName,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let maybe_dns_name = match server_name {
            ServerName::DnsName(dns) => Some(dns.as_ref()),
            _ => None,
        };

        match maybe_dns_name {
            // Verify remote attestation cert when provisioning node
            Some(dns_name)
                if dns_name.ends_with(constants::NODE_PROVISION_DNS_SUFFIX) =>
                self.attestation_verifier.verify_server_cert(
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
            &lexe_tls_core::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        lexe_tls_core::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
    }
}
