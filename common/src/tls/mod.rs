use anyhow::Context;
use rustls::{client::WebPkiVerifier, RootCertStore};

/// (m)TLS based on SGX remote attestation.
pub mod attestation;
/// mTLS based on a shared `RootSeed`.
pub mod shared_seed;

pub fn lexe_verifier(
    lexe_ca_cert: &rustls::Certificate,
) -> anyhow::Result<WebPkiVerifier> {
    let mut lexe_roots = RootCertStore::empty();
    lexe_roots
        .add(lexe_ca_cert)
        .context("Failed to deserialize lexe trust anchor")?;
    // TODO(phlip9): our web-facing certs will actually support cert
    // transparency
    let lexe_ct_policy = None;
    let lexe_verifier = WebPkiVerifier::new(lexe_roots, lexe_ct_policy);
    Ok(lexe_verifier)
}

// TODO(phlip9): need to replace this when we get the Lexe CA certs wired
// through
pub(crate) fn dummy_lexe_ca_cert() -> rustls::Certificate {
    let dns_names = vec!["localhost".to_owned()];
    let cert_params = rcgen::CertificateParams::new(dns_names);
    let fake_lexe_cert = rcgen::Certificate::from_params(cert_params).unwrap();
    rustls::Certificate(fake_lexe_cert.serialize_der().unwrap())
}
