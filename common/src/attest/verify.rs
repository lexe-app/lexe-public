//! Verify remote attestation endorsements directly or embedded in x509 certs.

use std::time::SystemTime;

use asn1_rs::FromDer;
use x509_parser::certificate::X509Certificate;

use crate::attest::cert::SgxAttestationExtension;
use crate::ed25519;

pub struct AttestEvidence<'a> {
    pub cert_pubkey: ed25519::PublicKey,
    pub attest: SgxAttestationExtension<'a, 'a>,
}

impl<'a> AttestEvidence<'a> {
    pub fn parse_cert_der(cert_der: &'a [u8]) -> Result<Self, rustls::Error> {
        use rustls::Error;

        // TODO(phlip9): manually parse the cert fields we care about w/ yasna
        // instead of pulling in a whole extra x509 cert parser...

        let (unparsed_data, cert) = X509Certificate::from_der(cert_der)
            .map_err(|err| Error::InvalidCertificateData(err.to_string()))?;

        if !unparsed_data.is_empty() {
            return Err(Error::InvalidCertificateData(
                "leftover unparsed cert data".to_string(),
            ));
        }

        let cert_pubkey = ed25519::PublicKey::try_from(cert.public_key())
            .map_err(|err| Error::InvalidCertificateData(err.to_string()))?;

        let sgx_ext_oid = SgxAttestationExtension::oid_asn1_rs();
        let cert_ext = cert
            .get_extension_unique(&sgx_ext_oid)
            .map_err(|err| Error::InvalidCertificateData(err.to_string()))?
            .ok_or_else(|| {
                Error::InvalidCertificateData(
                    "no SGX attestation extension".to_string(),
                )
            })?;

        let attest = SgxAttestationExtension::from_der_bytes(cert_ext.value)
            .map_err(|err| {
                Error::InvalidCertificateData(format!(
                    "invalid SGX attestation: {err}"
                ))
            })?;

        Ok(Self {
            cert_pubkey,
            attest,
        })
    }
}

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

        // TODO(phlip9): parse quote

        // in addition to the typical cert checks, we also need to extract
        // the enclave attestation quote from the cert and verify that.
        let evidence = AttestEvidence::parse_cert_der(&end_entity.0)?;

        if !self.expect_dummy_quote {
            // 4. (if not dev mode) parse out quote and quote report
            // 5. (if not dev mode) ensure report contains the pubkey hash
            // 6. (if not dev mode) verify quote and quote report
            todo!()
        } else if evidence.attest != SgxAttestationExtension::dummy() {
            return Err(rustls::Error::InvalidCertificateData(
                "invalid SGX attestation".to_string(),
            ));
        }

        Ok(verified_token)
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;
    use std::time::Duration;
    use std::{include_str, iter};

    use rustls::client::ServerCertVerifier as _;

    use super::*;
    use crate::attest::cert::{AttestationCert, SgxAttestationExtension};
    use crate::rng::SysRng;
    use crate::{ed25519, hex};

    const MRENCLAVE_HEX: &str = include_str!("../../test_data/mrenclave.hex");
    const SGX_SERVER_CERT_PEM: &str =
        include_str!("../../test_data/attest_cert.pem");

    fn parse_cert_pem_as_der(s: &str) -> Vec<u8> {
        let mut cursor = Cursor::new(s.as_bytes());

        let item = rustls_pemfile::read_one(&mut cursor)
            .expect("Expected at least one entry in the PEM file")
            .expect("Not valid PEM-encoded cert");

        match item {
            rustls_pemfile::Item::X509Certificate(der) => der,
            _ => panic!("Not an x509 certificate: {:?}", item),
        }
    }

    #[test]
    fn test_verify_sgx_server_cert() {
        let cert_der = parse_cert_pem_as_der(SGX_SERVER_CERT_PEM);
        let _evidence = AttestEvidence::parse_cert_der(&cert_der).unwrap();
        let _expected_mrenclave = hex::decode(MRENCLAVE_HEX.trim()).unwrap();
    }

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
