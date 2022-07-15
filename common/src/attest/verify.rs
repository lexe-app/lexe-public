//! Verify remote attestation endorsements directly or embedded in x509 certs.

use std::io::Cursor;
use std::time::SystemTime;
use std::{fmt, include_bytes};

use anyhow::{bail, ensure, format_err, Context, Result};
use asn1_rs::FromDer;
use dcap_ql::quote::{
    Qe3CertDataPckCertChain, Quote, Quote3SignatureEcdsaP256,
};
use once_cell::sync::Lazy;
use webpki::{TlsServerTrustAnchors, TrustAnchor};
use x509_parser::certificate::X509Certificate;

use crate::attest::cert::SgxAttestationExtension;
use crate::{ed25519, sha256};

/// The DER-encoded Intel SGX trust anchor cert.
const INTEL_SGX_ROOT_CA_CERT_DER: &[u8] =
    include_bytes!("../../data/intel-sgx-root-ca.der");

/// Lazily parse the Intel SGX trust anchor cert.
///
/// NOTE: It's easier to inline the cert DER bytes vs. PEM file, otherwise the
/// `TrustAnchor` tries to borrow from a temporary `Vec<u8>`.
static INTEL_SGX_TRUST_ANCHOR: Lazy<[TrustAnchor<'static>; 1]> =
    Lazy::new(|| {
        let trust_anchor = TrustAnchor::try_from_cert_der(
            INTEL_SGX_ROOT_CA_CERT_DER,
        )
        .expect("Failed to deserialize Intel SGX root CA cert from der bytes");

        [trust_anchor]
    });

/// From: <https://github.com/rustls/rustls/blob/v/0.20.6/rustls/src/verify.rs#L22>
static SUPPORTED_SIG_ALGS: &[&webpki::SignatureAlgorithm] = &[
    &webpki::ECDSA_P256_SHA256,
    &webpki::ECDSA_P256_SHA384,
    &webpki::ECDSA_P384_SHA256,
    &webpki::ECDSA_P384_SHA384,
    &webpki::ED25519,
    &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    &webpki::RSA_PKCS1_2048_8192_SHA256,
    &webpki::RSA_PKCS1_2048_8192_SHA384,
    &webpki::RSA_PKCS1_2048_8192_SHA512,
    &webpki::RSA_PKCS1_3072_8192_SHA384,
];

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

pub struct SgxQuoteVerifier {
    pub now: SystemTime,
}

impl SgxQuoteVerifier {
    pub fn verify(
        &self,
        quote_bytes: &[u8],
        _qe3_report: &[u8],
    ) -> Result<sgx_isa::Report> {
        let quote = Quote::parse(quote_bytes)
            .map_err(DisplayErr::new)
            .context("Failed to parse SGX Quote")?;

        // Only support DCAP + ECDSA P256 for now

        let sig = quote
            .signature::<Quote3SignatureEcdsaP256>()
            .map_err(DisplayErr::new)
            .context("Failed to parse SGX ECDSA Quote signature")?;

        let cert_chain_pem = sig
            .certification_data::<Qe3CertDataPckCertChain>()
            .map_err(DisplayErr::new)
            .context("Failed to parse PCK cert chain")?
            .certs;

        // 1. Verify the local SGX platform PCK cert is endorsed by the Intel
        //    SGX trust root CA.

        // [0] : PCK cert
        // [1] : PCK platform cert
        // [2] : SGX root CA cert
        ensure!(
            cert_chain_pem.len() == 3,
            "unexpected number of certificates"
        );

        let pck_cert_der = parse_cert_pem_to_der(&cert_chain_pem[0])
            .context("Failed to parse PCK cert PEM string")?;
        let pck_platform_cert_der =
            parse_cert_pem_to_der(&cert_chain_pem[1])
                .context("Failed to parse PCK platform cert PEM string")?;

        let pck_cert = webpki::EndEntityCert::try_from(pck_cert_der.as_slice())
            .context("Invalid PCK cert")?;

        let now = webpki::Time::try_from(self.now)
            .context("Our time source is bad")?;

        // We don't use the full `rustls::WebPkiVerifier` here since the PCK
        // certs aren't truly webpki certs, as they don't bind to DNS names.
        //
        // Instead, we skip the DNS checks by using the lower-level `EndEntity`
        // verification methods directly.

        pck_cert
            .verify_is_valid_tls_server_cert(
                SUPPORTED_SIG_ALGS,
                &TlsServerTrustAnchors(&*INTEL_SGX_TRUST_ANCHOR),
                &[&pck_platform_cert_der],
                now,
            )
            .context("PCK cert chain failed validation")?;

        let qe3_sig = get_ecdsa_sig_der(sig.qe3_signature())?;
        let qe3_report_bytes = sig.qe3_report();

        // 2. Verify the Platform Certification Enclave (PCE) endorses the
        //    Quoting Enclave (QE) Report.

        // TODO(phlip9): parse PCK cert pubkey + algorithm?
        pck_cert
            .verify_signature(
                &webpki::ECDSA_P256_SHA256,
                qe3_report_bytes,
                &qe3_sig,
            )
            .context(
                "PCK cert's signature on the Quoting Enclave Report is invalid",
            )?;

        // 3. Verify the local Quoting Enclave's Report binds to its attestation
        //    pubkey, which it uses to sign application enclave Reports.

        // expected_report_data :=
        //   SHA-256(attestation_public_key || authentication_data)
        let expected_report_data = sha256::digest_many(&[
            sig.attestation_public_key(),
            sig.authentication_data(),
        ]);

        let qe3_report = report_try_from_truncated(qe3_report_bytes)
            .context("Invalid QE Report")?;

        ensure!(
            &qe3_report.reportdata[..32] == expected_report_data.as_ref(),
            "Quoting Enclave's Report data doesn't match the Quote attestation pubkey",
        );
        ensure!(
            qe3_report.reportdata[32..] == [0u8; 32],
            "Quoting Enclave's Report contains unrecognized data",
        );

        // TODO(phlip9): verify QE identity

        // 4. Verify the attestation key endorses the Quote Header and our
        //    application enclave Rport

        let attestation_public_key =
            read_attestation_pubkey(sig.attestation_public_key())?;

        // msg := Quote Header || Application Enclave Report
        let msg_len = 432;
        ensure!(quote_bytes.len() >= msg_len, "Quote malformed");
        let msg = &quote_bytes[..432];

        attestation_public_key.verify(msg, sig.signature())
            .map_err(|_| format_err!("QE signature on application enclave report failed to verify"))?;

        // 5. Return the endorsed application enclave report

        let report = report_try_from_truncated(quote.report_body())
            .context("Invalid application enclave Report")?;

        Ok(report)
    }
}

// dumb error type compatibility hack so we can propagate `failure::Fail` errors

#[derive(Debug)]
struct DisplayErr(String);

impl DisplayErr {
    fn new(err: impl fmt::Display) -> Self {
        Self(format!("{:#}", err))
    }
}

impl fmt::Display for DisplayErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DisplayErr {
    fn description(&self) -> &str {
        &self.0
    }
}

fn parse_cert_pem_to_der(s: &str) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(s.as_bytes());

    let item = rustls_pemfile::read_one(&mut cursor)
        .context("Expected at least one entry in the PEM file")?
        .ok_or_else(|| format_err!("Not valid PEM-encoded cert"))?;

    match item {
        rustls_pemfile::Item::X509Certificate(der) => Ok(der),
        _ => bail!("Not an x509 certificate PEM label"),
    }
}

/// Convert a (presumably) fixed `r || s` ECDSA signature to ASN.1 format
///
/// From: <https://github.com/fortanix/rust-sgx/blob/3fea4337f774fe9563a62352ce62d3cad7af746d/intel-sgx/dcap-ql/src/quote.rs#L212>
/// See: [RFC 3279 2.2.3](https://datatracker.ietf.org/doc/html/rfc3279#section-2.2.3)
///
/// ```asn.1
/// Ecdsa-Sig-Value ::= SEQUENCE {
///     r INTEGER,
///     s INTEGER
/// }
/// ```
fn get_ecdsa_sig_der(sig: &[u8]) -> Result<Vec<u8>> {
    if sig.len() % 2 != 0 {
        bail!("sig not even: {}", sig.len());
    }

    // TODO(phlip9): avoid `num` dependency...
    let (r_bytes, s_bytes) = sig.split_at(sig.len() / 2);
    let r = num_bigint::BigUint::from_bytes_be(r_bytes);
    let s = num_bigint::BigUint::from_bytes_be(s_bytes);

    let der = yasna::construct_der(|writer| {
        writer.write_sequence(|writer| {
            writer.next().write_biguint(&r);
            writer.next().write_biguint(&s);
        })
    });

    Ok(der)
}

fn read_attestation_pubkey(
    bytes: &[u8],
) -> Result<ring::signature::UnparsedPublicKey<[u8; 65]>> {
    ensure!(bytes.len() == 64, "Attestation public key is in an unrecognized format; expected exactly 64 bytes, actual len: {}", bytes.len());

    let mut attestation_public_key = [0u8; 65];
    attestation_public_key[0] = 0x4;
    attestation_public_key[1..].copy_from_slice(bytes);

    Ok(ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_FIXED,
        attestation_public_key,
    ))
}

fn report_try_from_truncated(bytes: &[u8]) -> Result<sgx_isa::Report> {
    use sgx_isa::Report;

    let len = bytes.len();
    let expected_len = Report::TRUNCATED_SIZE;
    ensure!(
        len == expected_len,
        "report has the wrong size: {len}, expected: {expected_len}",
    );

    let mut unpadded = vec![0u8; Report::UNPADDED_SIZE];
    unpadded[..Report::TRUNCATED_SIZE].copy_from_slice(bytes);

    Ok(Report::try_copy_from(&unpadded).expect("Should never fail"))
}

#[cfg(test)]
mod test {
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

    const INTEL_SGX_ROOT_CA_CERT_PEM: &str =
        include_str!("../../test_data/intel-sgx-root-ca.pem");

    #[test]
    fn test_intel_sgx_trust_anchor_der_pem_equal() {
        let sgx_trust_anchor_der = INTEL_SGX_ROOT_CA_CERT_DER;
        let sgx_trust_anchor_pem =
            parse_cert_pem_to_der(INTEL_SGX_ROOT_CA_CERT_PEM).unwrap();
        assert_eq!(sgx_trust_anchor_der, sgx_trust_anchor_pem);

        // this should not panic
        let _sgx_trust_anchor = &*INTEL_SGX_TRUST_ANCHOR;
    }

    #[test]
    fn test_verify_sgx_server_cert() {
        let cert_der = parse_cert_pem_to_der(SGX_SERVER_CERT_PEM).unwrap();
        let evidence = AttestEvidence::parse_cert_der(&cert_der).unwrap();
        let expected_mrenclave = hex::decode(MRENCLAVE_HEX.trim()).unwrap();

        let verifier = SgxQuoteVerifier {
            now: SystemTime::now(),
        };
        let report = verifier
            .verify(&evidence.attest.quote, &evidence.attest.qe_report)
            .unwrap();

        assert_eq!(report.mrenclave.as_slice(), expected_mrenclave);
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
