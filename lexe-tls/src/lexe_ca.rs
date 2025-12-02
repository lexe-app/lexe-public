//! Certs and utilities related to Lexe's CA.

use std::sync::{Arc, LazyLock};

use common::{constants, ed25519, env::DeployEnv};
use rustls::{
    RootCertStore,
    client::WebPkiServerVerifier,
    server::{WebPkiClientVerifier, danger::ClientCertVerifier},
};

use super::types::{CertWithKey, LxCertificateDer, LxPrivatePkcs8KeyDer};

/// Client-side TLS config for app->gateway APIs, i.e. the `GatewayClient`.
/// This TLS config covers:
/// - `AppGatewayApi`
/// - `AppBackendApi`
/// - `BearerAuthBackendApi` for the app
///
/// It does *not* cover the gateway's node proxy.
pub fn app_gateway_client_config(
    deploy_env: DeployEnv,
) -> rustls::ClientConfig {
    // Only trust Lexe's CA, no WebPKI roots, no client auth.
    let lexe_verifier = lexe_server_verifier(deploy_env);
    let mut config = lexe_tls_core::client_config_builder()
        .with_webpki_verifier(lexe_verifier)
        .with_no_client_auth();
    config
        .alpn_protocols
        .clone_from(&lexe_tls_core::LEXE_ALPN_PROTOCOLS);

    config
}

/// Get the appropriate DER-encoded Lexe CA cert for this deploy environment.
pub fn lexe_ca_cert(deploy_env: DeployEnv) -> LxCertificateDer {
    match deploy_env {
        DeployEnv::Dev =>
            LxCertificateDer(constants::LEXE_DUMMY_CA_CERT_DER.to_vec()),
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
        lexe_tls_core::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .expect("Checked in tests")
}

/// Get a [`ClientCertVerifier`] which verifies that a presented client cert has
/// been signed by Lexe's CA (without trusting Mozilla's WebPKI roots).
pub fn lexe_client_verifier(
    deploy_env: DeployEnv,
) -> Arc<dyn ClientCertVerifier + 'static> {
    let lexe_ca_cert = lexe_ca_cert(deploy_env);

    let mut roots = RootCertStore::empty();
    roots.add(lexe_ca_cert.into()).expect("Checked in tests");

    WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        lexe_tls_core::LEXE_CRYPTO_PROVIDER.clone(),
    )
    .build()
    .expect("Checked in tests")
}

pub fn dummy_lexe_ca_key_pair() -> &'static ed25519::KeyPair {
    static DUMMY_LEXE_CA_KEY_PAIR: LazyLock<ed25519::KeyPair> =
        LazyLock::new(|| ed25519::KeyPair::from_seed_owned([69; 32]));
    &DUMMY_LEXE_CA_KEY_PAIR
}

/// Get a dummy Lexe CA cert along with its corresponding private key.
pub fn dummy_lexe_ca_cert() -> CertWithKey {
    let dummy_key_pair = dummy_lexe_ca_key_pair();
    let dummy_cert_key_der =
        LxPrivatePkcs8KeyDer(dummy_key_pair.serialize_pkcs8_der().to_vec());
    let dummy_cert_der =
        LxCertificateDer(constants::LEXE_DUMMY_CA_CERT_DER.to_vec());
    CertWithKey {
        cert_der: dummy_cert_der,
        cert_chain_der: vec![],
        key_der: dummy_cert_key_der,
    }
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, proptest, test_runner::Config};

    use super::*;

    /// Generate a dummy Lexe CA cert along with its corresponding private key.
    fn gen_dummy_lexe_ca_cert() -> CertWithKey {
        let dummy_key_pair = dummy_lexe_ca_key_pair();
        let dummy_cert_params = crate::build_rcgen_cert_params(
            "Lexe CA cert",
            rcgen::date_time_ymd(1975, 1, 1),
            rcgen::date_time_ymd(4096, 1, 1),
            crate::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            dummy_key_pair.public_key(),
            |params: &mut rcgen::CertificateParams| {
                params.is_ca =
                    rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
                params.name_constraints = None;
            },
        );

        let issuer =
            rcgen::Issuer::from_params(&dummy_cert_params, &dummy_key_pair);

        let dummy_cert = dummy_cert_params
            .signed_by(&dummy_key_pair, &issuer)
            .unwrap();
        let dummy_cert_der = LxCertificateDer(dummy_cert.der().to_vec());
        let dummy_cert_key_der =
            LxPrivatePkcs8KeyDer(dummy_key_pair.serialize_pkcs8_der().to_vec());

        CertWithKey {
            cert_der: dummy_cert_der,
            cert_chain_der: vec![],
            key_der: dummy_cert_key_der,
        }
    }

    #[test]
    fn verifier_helpers_dont_panic() {
        let config = Config::with_cases(4);
        proptest!(config, |(deploy_env in any::<DeployEnv>())| {
            lexe_server_verifier(deploy_env);
            lexe_client_verifier(deploy_env);
        })
    }

    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn test_dummy_lexe_ca_cert_eq() {
        let cert1 = gen_dummy_lexe_ca_cert().cert_der.0;
        let cert2 = std::fs::read("../common/data/lexe-dummy-root-ca-cert.der")
            .unwrap();
        assert_eq!(
            cert1, cert2,
            "The generated dummy Lexe CA cert doesn't match the checked in \
             version. Regenerate it:\n\
             \n\
             $ cargo test -p lexe-tls -- --ignored dump_dummy_lexe_ca\n\
             \n"
        );
    }

    /// ```bash
    /// $ cargo test -p lexe-tls -- --ignored dump_dummy_lexe_ca_cert
    /// ```
    #[ignore]
    #[test]
    fn dump_dummy_lexe_ca_cert() {
        let cert = gen_dummy_lexe_ca_cert().cert_der;
        std::fs::write(
            "../common/data/lexe-dummy-root-ca-cert.new.der",
            &cert.0,
        )
        .unwrap();
    }
}
