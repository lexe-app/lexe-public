//! (m)TLS based on SGX remote attestation.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

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
#[cfg(doc)]
use crate::api::def::{
    BearerAuthBackendApi, NodeBackendApi, NodeLspApi, NodeRunnerApi,
};
use crate::{
    constants,
    enclave::{Measurement, MrShort},
    env::DeployEnv,
    rng::Crng,
    tls::{lexe_ca, CertWithKey},
};

/// Self-signed x509 cert containing enclave remote attestation endorsements.
pub mod cert;
/// Get a quote for the running node enclave.
pub mod quote;
/// Verify remote attestation endorsements directly or embedded in x509 certs.
pub mod verifier;

/// Server-side TLS config for [`AppNodeProvisionApi`].
/// Also returns the node's DNS name.
pub fn app_node_provision_server_config(
    rng: &mut impl Crng,
    measurement: &Measurement,
) -> anyhow::Result<(rustls::ServerConfig, String)> {
    let mr_short = measurement.short();
    let node_mode = NodeMode::Provision { mr_short };
    let (attestation_cert_with_key, dns_name) =
        get_or_generate_node_attestation_cert(rng, node_mode)
            .context("Failed to get or generate node attestation cert")?;
    let CertWithKey {
        cert_der: attestation_cert_der,
        key_der: attestation_cert_key_der,
    } = attestation_cert_with_key;

    let mut config = super::lexe_server_config()
        .with_no_client_auth()
        .with_single_cert(
            vec![attestation_cert_der.clone()],
            attestation_cert_key_der.clone_key(),
        )
        .context("Failed to build TLS config")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok((config, dns_name))
}

/// Client-side TLS config for [`AppNodeProvisionApi`].
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
    let attestation_verifier = verifier::AttestationServerVerifier {
        expect_dummy_quote: !use_sgx,
        enclave_policy,
    };
    let lexe_server_verifier = lexe_ca::lexe_server_verifier(deploy_env);

    let server_cert_verifier = AppNodeProvisionVerifier {
        lexe_server_verifier,
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

/// Client-side TLS config for node->Lexe APIs. This TLS config covers:
/// - [`NodeBackendApi`]
/// - [`NodeLspApi`]
/// - [`NodeRunnerApi`]
/// - [`BearerAuthBackendApi`] for the node
pub fn node_lexe_client_config(
    rng: &mut impl Crng,
    deploy_env: DeployEnv,
    node_mode: NodeMode,
) -> anyhow::Result<rustls::ClientConfig> {
    // Only trust Lexe's CA, no WebPKI roots, no client auth.
    let lexe_server_verifier = lexe_ca::lexe_server_verifier(deploy_env);

    // Authenticate ourselves using remote attestation.
    let (attestation_cert_with_key, _) =
        get_or_generate_node_attestation_cert(rng, node_mode)
            .context("Failed to get or generate node attestation cert")?;
    let CertWithKey {
        cert_der: attestation_cert_der,
        key_der: attestation_cert_key_der,
    } = attestation_cert_with_key;

    let mut config = super::lexe_client_config()
        .with_webpki_verifier(lexe_server_verifier)
        .with_client_auth_cert(
            vec![attestation_cert_der.clone()],
            attestation_cert_key_der.clone_key(),
        )
        .context("Failed to build TLS config")?;
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    Ok(config)
}

/// The mode that the user node is currently running in, and associated info.
#[derive(Copy, Clone)]
pub enum NodeMode {
    Provision { mr_short: MrShort },
    Run,
}

/// A helper to get or generate a remote attestation TLS cert for the user node.
/// This function prevents the user node from generating multiple (duplicate)
/// remote attestations during a single node lifetime.
/// Additionally returns the DNS name that the cert was bound to.
fn get_or_generate_node_attestation_cert(
    rng: &mut impl Crng,
    node_mode: NodeMode,
) -> anyhow::Result<(&CertWithKey, String)> {
    // Determine the cert lifetime. Until we can refresh the TLS cert during
    // runtime, this has to be longer than the time between node restarts.
    let lifetime = match node_mode {
        // Long lifetime (3 months); Provision nodes restart once every deploy.
        NodeMode::Provision { .. } => Duration::from_secs(60 * 60 * 24 * 90),
        // Medium lifetime; Run nodes restart fairly frequently.
        NodeMode::Run => Duration::from_secs(60 * 60 * 24 * 14), // 2 weeks
    };

    // The DNS name to bind the remote attestation cert to. Currently only
    // useful for a provisioning node which embeds the remote attestation
    // evidence in its server cert. For Node->Lexe TLS (used in both run and
    // provision mode), the attestation evidence is embedded in a client cert,
    // where the DNS name doesn't matter.
    let dns_name = match node_mode {
        NodeMode::Provision { mr_short } =>
            constants::node_provision_dns(&mr_short),
        NodeMode::Run => constants::NODE_RUN_DNS.to_owned(),
    };

    // Only generate a remote attestation cert once during a node's lifetime.
    // Subsequent calls will reuse the cert (and its key).
    static ATTESTATION_CERT: OnceLock<CertWithKey> = OnceLock::new();

    let attestation_cert_with_key = ATTESTATION_CERT
        .get_or_try_init(|| {
            let cert = cert::AttestationCert::generate(
                rng,
                dns_name.clone(),
                lifetime,
            )
            .context("Could not generate remote attestation cert")?;
            let cert_der = cert
                .serialize_der_self_signed()
                .context("Failed to sign and serialize attestation cert")?;
            let cert_key_der = cert.serialize_key_der();
            let cert_with_key = CertWithKey {
                cert_der,
                key_der: cert_key_der,
            };

            Ok::<_, anyhow::Error>(cert_with_key)
        })
        .context("Couldn't get or init attestation cert")?;

    Ok((attestation_cert_with_key, dns_name))
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
    attestation_verifier: verifier::AttestationServerVerifier,
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
            &super::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        super::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
    }
}
