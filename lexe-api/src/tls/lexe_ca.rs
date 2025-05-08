//! Certs and utilities related to Lexe's CA.

use std::sync::Arc;

#[cfg(doc)]
use common::api::def::{AppBackendApi, AppGatewayApi, BearerAuthBackendApi};
use common::{constants, ed25519, env::DeployEnv};
use rustls::{
    client::WebPkiServerVerifier,
    server::{danger::ClientCertVerifier, WebPkiClientVerifier},
    RootCertStore,
};

use super::types::{CertWithKey, LxCertificateDer, LxPrivatePkcs8KeyDer};

/// Client-side TLS config for app->gateway APIs, i.e. the `GatewayClient`.
/// This TLS config covers:
/// - [`AppGatewayApi`]
/// - [`AppBackendApi`]
/// - [`BearerAuthBackendApi`] for the app
///
/// It does *not* cover the gateway's node proxy.
pub fn app_gateway_client_config(
    deploy_env: DeployEnv,
) -> rustls::ClientConfig {
    // Only trust Lexe's CA, no WebPKI roots, no client auth.
    let lexe_verifier = lexe_server_verifier(deploy_env);
    let mut config = super::client_config_builder()
        .with_webpki_verifier(lexe_verifier)
        .with_no_client_auth();
    config
        .alpn_protocols
        .clone_from(&super::LEXE_ALPN_PROTOCOLS);

    config
}

/// Get the appropriate DER-encoded Lexe CA cert for this deploy environment.
pub fn lexe_ca_cert(deploy_env: DeployEnv) -> LxCertificateDer {
    match deploy_env {
        DeployEnv::Dev => dummy_lexe_ca_cert().cert_der,
        DeployEnv::Staging =>
            LxCertificateDer(constants::LEXE_STAGING_CA_CERT_DER.to_vec()),
        DeployEnv::Prod =>
            LxCertificateDer(constants::LEXE_PROD_CA_CERT_DER.to_vec()),
    }
}

/// Get a [`ServerCertVerifier`] which verifies that a presented server cert has
/// been signed by Lexe's CA (without trusting Mozilla's WebPKI roots).
///
/// [`ServerCertVerifier`]: rustls::client::danger::ServerCertVerifier
pub fn lexe_server_verifier(
    deploy_env: DeployEnv,
) -> Arc<WebPkiServerVerifier> {
    let lexe_ca_cert = lexe_ca_cert(deploy_env);

    let mut lexe_roots = RootCertStore::empty();
    lexe_roots
        .add(lexe_ca_cert.into())
        .expect("Checked in tests");
    WebPkiServerVerifier::builder_with_provider(
        Arc::new(lexe_roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .expect("Checked in tests")
}

/// Get a [`ClientCertVerifier`] which verifies that a presented client cert has
/// been signed by Lexe's CA (without trusting Mozilla's WebPKI roots).
pub fn lexe_client_verifier(
    deploy_env: DeployEnv,
) -> Arc<(dyn ClientCertVerifier + 'static)> {
    let lexe_ca_cert = lexe_ca_cert(deploy_env);

    let mut roots = RootCertStore::empty();
    roots.add(lexe_ca_cert.into()).expect("Checked in tests");

    WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        super::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .expect("Checked in tests")
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
        dummy_cert.serialize_der().map(LxCertificateDer).unwrap();
    let dummy_cert_key_der =
        LxPrivatePkcs8KeyDer(dummy_cert.serialize_private_key_der());

    CertWithKey {
        cert_der: dummy_cert_der,
        key_der: dummy_cert_key_der,
        ca_cert_der: None,
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
            lexe_server_verifier(deploy_env);
            lexe_client_verifier(deploy_env);
        })
    }
}
