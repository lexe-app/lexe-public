//! (m)TLS based on SGX remote attestation.

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
    DigitallySignedStruct,
};

use self::verifier::EnclavePolicy;
#[cfg(doc)]
use crate::api::def::AppNodeProvisionApi;
use crate::{
    constants, enclave::Measurement, env::DeployEnv, rng::Crng, tls::lexe_ca,
};

/// Self-signed x509 cert containing enclave remote attestation endorsements.
pub mod cert;
/// Get a quote for the running node enclave.
pub mod quote;
/// Verify remote attestation endorsements directly or embedded in x509 certs.
pub mod verifier;

/// Server-side TLS config for [`AppNodeProvisionApi`].
pub fn app_node_provision_server_config(
    rng: &mut impl Crng,
    measurement: &Measurement,
) -> anyhow::Result<rustls::ServerConfig> {
    // Bind the remote attestation cert to the node provision dns.
    let mr_short = measurement.short();
    let dns_name = constants::node_provision_dns(&mr_short).to_owned();

    let cert = cert::AttestationCert::generate(rng, dns_name)
        .context("Could not generate remote attestation cert")?;
    let cert_der = cert
        .serialize_der_self_signed()
        .context("Failed to sign and serialize attestation cert")?;
    let cert_key_der = cert.serialize_key_der();

    let mut config = super::lexe_server_config()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], cert_key_der)
        .context("Failed to build TLS config")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok(config)
}

/// Client-side TLS config for [`AppNodeProvisionApi`].
pub fn app_node_provision_client_config(
    use_sgx: bool,
    deploy_env: DeployEnv,
    measurement: Measurement,
) -> rustls::ClientConfig {
    let enclave_policy = EnclavePolicy::trust_measurement_with_signer(
        use_sgx,
        deploy_env,
        measurement,
    );
    let attestation_verifier = verifier::AttestationVerifier {
        expect_dummy_quote: !use_sgx,
        enclave_policy,
    };
    let public_lexe_verifier = lexe_ca::public_lexe_verifier(deploy_env);

    let server_cert_verifier = AppNodeProvisionVerifier {
        public_lexe_verifier,
        attestation_verifier,
    };

    let mut config = super::lexe_client_config()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(server_cert_verifier))
        .with_no_client_auth();
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    config
}

/// The client's [`ServerCertVerifier`] for [`AppNodeProvisionApi`] TLS.
///
/// - When the app wishes to provision, it will make a request to the node using
///   a fake provision DNS given by [`constants::node_provision_dns`]. However,
///   requests are first routed through lexe's reverse proxy, which parses the
///   fake provision DNS in the SNI extension to determine (1) whether we want
///   to connect to a running or provisioning node and (2) the [`MrShort`] of
///   the measurement we wish to provision so it can route accordingly.
/// - The [`ServerName`] is given by the [`NodeClient`] reqwest client. This is
///   the gateway DNS when connecting to Lexe's proxy, otherwise it is the
///   node's fake provision DNS. See [`NodeClient::provision`] for details.
/// - The [`AppNodeProvisionVerifier`] thus chooses between two "sub-verifiers"
///   according to the [`ServerName`] given to us by [`reqwest`]. We use the
///   public Lexe WebPKI verifier when establishing the outer TLS connection
///   with the gateway, and we use the remote attestation verifier for the inner
///   TLS connection which terminates inside the user node SGX enclave.
///
/// [`MrShort`]: crate::enclave::MrShort
/// [`NodeClient`]: crate::client::NodeClient
/// [`NodeClient::provision`]: crate::client::NodeClient::provision
#[derive(Debug)]
struct AppNodeProvisionVerifier {
    /// `<mr_short>.provision.lexe.app` remote attestation verifier
    attestation_verifier: verifier::AttestationVerifier,
    /// `<TODO>.lexe.app` Lexe reverse proxy verifier - trusts the Lexe CA
    public_lexe_verifier: Arc<WebPkiServerVerifier>,
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
            // Other domains (i.e., node reverse proxy) verify using pinned
            // lexe CA
            // TODO(phlip8): this should be a strict DNS name, like
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
