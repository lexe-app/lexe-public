//! Verify remote attestation endorsements directly or embedded in x509 certs.

use std::time::SystemTime;

use asn1_rs::FromDer;
use x509_parser::certificate::X509Certificate;

use crate::attest::cert::SgxAttestationExtension;
use crate::ed25519;

// 1. server cert verifier (server cert should contain dns names)
// 2. TODO(phlip9): client cert verifier (dns names ignored)

/// An x509 certificate verifier that also checks embedded remote attestation
/// evidence.
///
/// Clients use this verifier to check that
/// (1) a server's certificate is valid,
/// (2) the remote attestation is valid (according to the client's policy), and
/// (3) the remote attestation binds to the server's certificate key pair. Once
/// these checks are successful, the client and secure can establish a secure
/// TLS channel.
#[derive(Default)]
pub struct ServerCertVerifier {
    pub expect_dummy_quote: bool,
}

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
        // there should be no intermediate certs
        if !intermediates.is_empty() {
            return Err(rustls::Error::General(
                "received unexpected intermediate certs".to_owned(),
            ));
        }

        // verify the self-signed cert "normally"; ensure everything is
        // well-formed, signatures verify, validity ranges OK, SNI matches,
        // etc...
        let mut trust_roots = rustls::RootCertStore::empty();
        trust_roots.add(end_entity).map_err(|err| {
            rustls::Error::InvalidCertificateData(err.to_string())
        })?;

        let ct_policy = None;
        let webpki_verifier =
            rustls::client::WebPkiVerifier::new(trust_roots, ct_policy);

        let verified_token = webpki_verifier.verify_server_cert(
            end_entity,
            &[],
            server_name,
            scts,
            ocsp_response,
            now,
        )?;

        // in addition to the typical cert checks, we also need to extract
        // the enclave attestation quote from the cert and verify that.

        // TODO(phlip9): parse quote

        let (_, cert) =
            X509Certificate::from_der(&end_entity.0).map_err(|err| {
                rustls::Error::InvalidCertificateData(err.to_string())
            })?;

        // TODO(phlip9): check binding b/w cert pubkey and Quote report data
        let _cert_pubkey = ed25519::PublicKey::try_from(cert.public_key())
            .map_err(|err| {
                rustls::Error::InvalidCertificateData(err.to_string())
            })?;

        let sgx_ext_oid = SgxAttestationExtension::oid_asn1_rs();
        let cert_ext = cert
            .get_extension_unique(&sgx_ext_oid)
            .map_err(|err| {
                rustls::Error::InvalidCertificateData(err.to_string())
            })?
            .ok_or_else(|| {
                rustls::Error::InvalidCertificateData(
                    "no SGX attestation extension".to_string(),
                )
            })?;

        let attest = SgxAttestationExtension::from_der_bytes(cert_ext.value)
            .map_err(|err| {
                rustls::Error::InvalidCertificateData(format!(
                    "invalid SGX attestation: {err}"
                ))
            })?;

        if !self.expect_dummy_quote {
            // 4. (if not dev mode) parse out quote and quote report
            // 5. (if not dev mode) ensure report contains the pubkey hash
            // 6. (if not dev mode) verify quote and quote report
            todo!()
        } else if attest != SgxAttestationExtension::dummy() {
            return Err(rustls::Error::InvalidCertificateData(
                "invalid SGX attestation".to_string(),
            ));
        }

        Ok(verified_token)
    }
}

#[cfg(test)]
mod test {
    use std::iter;
    use std::time::Duration;

    use rustls::client::ServerCertVerifier as _;

    use super::*;
    use crate::attest::cert::{AttestationCert, SgxAttestationExtension};
    use crate::ed25519;
    use crate::rng::SysRng;

    #[test]
    fn test_verify_dummy_server_cert() {
        let mut rng = SysRng::new();

        let dns_name = "node.lexe.tech";
        let dns_names = vec![dns_name.to_owned()];

        let cert_key_pair = ed25519::gen_key_pair(&mut rng);
        let attestation = SgxAttestationExtension::dummy().to_cert_extension();
        let cert = AttestationCert::new(cert_key_pair, dns_names, attestation)
            .unwrap();
        let cert_der = cert.serialize_der_signed().unwrap();

        let verifier = ServerCertVerifier {
            expect_dummy_quote: true,
        };

        // some time in 2022 lol
        let now = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(1_650_000_000))
            .unwrap();

        let intermediates = &[];
        let mut scts = iter::empty();
        let ocsp_response = &[];

        verifier
            .verify_server_cert(
                &rustls::Certificate(cert_der),
                intermediates,
                &rustls::ServerName::try_from(dns_name).unwrap(),
                &mut scts,
                ocsp_response,
                now,
            )
            .unwrap();
    }
}
