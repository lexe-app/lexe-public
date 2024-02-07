//! Certs and utilities related to Lexe's CA.

use std::sync::Arc;

use rustls::{
    client::WebPkiVerifier,
    server::{AllowAnyAuthenticatedClient, ClientCertVerifier},
    RootCertStore,
};

use super::CertWithKey;
use crate::{ed25519, env::DeployEnv};

/// Get the appropriate DER-encoded Lexe CA cert for this deploy environment.
pub fn lexe_ca_cert(deploy_env: DeployEnv) -> rustls::Certificate {
    match deploy_env {
        DeployEnv::Dev => dummy_lexe_ca_cert().cert_der,
        DeployEnv::Prod => dummy_lexe_ca_cert().cert_der,
        DeployEnv::Staging => dummy_lexe_ca_cert().cert_der,
        // TODO(max): Switch to hard-coded certs in common::constants
        // DeployEnv::Staging =>
        //     rustls::Certificate(constants::LEXE_STAGING_CA_CERT_DER),
        // DeployEnv::Prod =>
        //     rustls::Certificate(constants::LEXE_PROD_CA_CERT_DER),
    }
}

/// Get a [`ServerCertVerifier`] which verifies that a presented server cert has
/// been signed by Lexe's CA (without trusting Mozilla's WebPKI roots).
///
/// This verifier enforces certificate transparency, so should only be used for
/// requests to Lexe infrastructure made over the public (external) Internet.
///
/// [`ServerCertVerifier`]: rustls::client::ServerCertVerifier
pub fn public_lexe_verifier(deploy_env: DeployEnv) -> WebPkiVerifier {
    let lexe_ca_cert = lexe_ca_cert(deploy_env);

    let mut lexe_roots = RootCertStore::empty();
    lexe_roots.add(&lexe_ca_cert).expect("Checked in tests");
    // TODO(phlip9): actually enforce cert transparency
    let lexe_ct_policy = None;
    WebPkiVerifier::new(lexe_roots, lexe_ct_policy)
}

/// Get a [`ClientCertVerifier`] which verifies that a presented client cert has
/// been signed by Lexe's CA (without trusting Mozilla's WebPKI roots).
pub fn lexe_client_verifier(
    deploy_env: DeployEnv,
) -> Arc<(dyn ClientCertVerifier + 'static)> {
    let lexe_ca_cert = lexe_ca_cert(deploy_env);

    let mut roots = RootCertStore::empty();
    roots.add(&lexe_ca_cert).expect("Checked in tests");

    AllowAnyAuthenticatedClient::new(roots)
}

/// Get a dummy Lexe CA cert along with its corresponding private key.
pub fn dummy_lexe_ca_cert() -> CertWithKey {
    let dummy_cert = super::build_rcgen_cert(
        "Lexe CA cert",
        rcgen::date_time_ymd(1975, 1, 1),
        rcgen::date_time_ymd(4096, 1, 1),
        super::DEFAULT_SUBJECT_ALT_NAMES.clone(),
        &ed25519::KeyPair::from_seed_owned([69; 32]),
        |params: &mut rcgen::CertificateParams| {
            params.is_ca =
                rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            params.name_constraints = None;
        },
    );
    let dummy_cert_der =
        dummy_cert.serialize_der().map(rustls::Certificate).unwrap();
    let dummy_cert_key_der =
        rustls::PrivateKey(dummy_cert.serialize_private_key_der());

    CertWithKey {
        cert_der: dummy_cert_der,
        key_der: dummy_cert_key_der,
    }
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, proptest, test_runner::Config};

    use super::*;

    #[test]
    fn verifier_helpers_dont_panic() {
        let config = Config::with_cases(4);
        proptest!(config, |(deploy_env in any::<DeployEnv>())| {
            public_lexe_verifier(deploy_env);
            lexe_client_verifier(deploy_env);
        })
    }
}
