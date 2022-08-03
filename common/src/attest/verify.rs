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
use crate::{ed25519, hex, sha256};

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
pub struct ServerCertVerifier {
    /// if `true`, expect a fake dummy quote. Used only for testing.
    pub expect_dummy_quote: bool,
    /// the verifier's policy for trusting the remote enclave.
    pub enclave_policy: EnclavePolicy,
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
            return Err(rustls_err("received unexpected intermediate certs"));
        }

        // 1. verify the self-signed cert "normally"; ensure everything is
        //    well-formed, signatures verify, validity ranges OK, SNI matches,
        //    etc...
        let mut trust_roots = rustls::RootCertStore::empty();
        trust_roots.add(end_entity).map_err(rustls_err)?;

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

        // 2. extract enclave attestation quote from the cert
        let evidence = AttestEvidence::parse_cert_der(&end_entity.0)?;

        if !self.expect_dummy_quote {
            // 3. verify Quote
            let quote_verifier = SgxQuoteVerifier;
            let enclave_report =
                quote_verifier.verify(&evidence.attest.quote, now).map_err(
                    |err| rustls_err(format!("invalid SGX Quote: {err:#}")),
                )?;

            // 4. decide if we can trust this enclave
            let reportdata =
                self.enclave_policy.verify(&enclave_report).map_err(|err| {
                    rustls_err(format!(
                        "our trust policy rejected the remote enclave: {err:#}"
                    ))
                })?;

            // 5. check that the pk in the enclave Report matches the one in
            //    this x509 cert.
            if &reportdata[..32] != evidence.cert_pk.as_bytes() {
                return Err(rustls_err(
                    "enclave's report is not actually binding to the presented x509 cert"
                ));
            }
        } else if evidence.attest != SgxAttestationExtension::dummy() {
            return Err(rustls_err("invalid SGX attestation"));
        }

        Ok(verified_token)
    }

    fn request_scts(&self) -> bool {
        false
    }
}

struct AttestEvidence<'a> {
    cert_pk: ed25519::PublicKey,
    attest: SgxAttestationExtension<'a, 'a>,
}

impl<'a> AttestEvidence<'a> {
    fn parse_cert_der(cert_der: &'a [u8]) -> Result<Self, rustls::Error> {
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

        let cert_pk = ed25519::PublicKey::try_from(cert.public_key())
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

        Ok(Self { cert_pk, attest })
    }
}

/// A verifier for validating SGX [`Quote`]s.
///
/// To read a more in-depth explanation of the verification logic and see some
/// pretty pictures showing the chain of trust from the Intel SGX root CA down
/// to the application enclave's ReportData, visit:
/// [phlip9.com/notes - SGX Remote Attestation Quote Verification](https://phlip9.com/notes/confidential%20computing/intel%20SGX/remote%20attestation/#remote-attestation-quote-verification)
struct SgxQuoteVerifier;

impl SgxQuoteVerifier {
    fn verify(
        &self,
        quote_bytes: &[u8],
        now: SystemTime,
    ) -> Result<sgx_isa::Report> {
        let quote = Quote::parse(quote_bytes)
            .map_err(DisplayErr::new)
            .context("Failed to parse SGX Quote")?;

        // TODO(phlip9): what is `quote.header().user_data` for?

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

        let now =
            webpki::Time::try_from(now).context("Our time source is bad")?;

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

        // TODO(phlip9): parse PCK cert pk + algorithm vs hard-coding scheme
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
        //    pk, which it uses to sign application enclave Reports.

        let expected_reportdata = sha256::digest_many(&[
            sig.attestation_public_key(),
            sig.authentication_data(),
        ]);

        let qe3_report = report_try_from_truncated(qe3_report_bytes)
            .context("Invalid QE Report")?;

        // TODO(phlip9): request QE identity from IAC?
        let qe3_reportdata = EnclavePolicy::trust_intel_qe()
            .verify(&qe3_report)
            .context("Invalid QE identity")?;

        ensure!(
            &qe3_reportdata[..32] == expected_reportdata.as_ref(),
            "Quoting Enclave's Report data doesn't match the Quote attestation pk: \
             actual: '{}', expected: '{}'",
            hex::display(&qe3_reportdata[..32]),
            hex::display(expected_reportdata.as_ref()),
        );

        // 4. Verify the attestation key endorses the Quote Header and our
        //    application enclave Report

        let attestation_public_key =
            read_attestation_pk(sig.attestation_public_key())?;

        // signature := Ecdsa-P256-SHA256-Sign_{AK}(
        //   Quote Header || Application Enclave Report
        // )
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
        .context("Not valid PEM-encoded cert")?
        .ok_or_else(|| format_err!("The PEM file contains no entries"))?;

    match item {
        rustls_pemfile::Item::X509Certificate(der) => Ok(der),
        _ => bail!("First entry in PEM file is not a cert"),
    }
}

// TODO(phlip9): expand functionality. parse+verify sig from QE3 Identity json
// and convert to an `EnclavePolicy`.
// TODO(phlip9): check `cpusvn`, `isvsvn`, `isvprodid`

/// A verifier's policy for which enclaves it should trust.
pub struct EnclavePolicy {
    /// Allow enclaves in DEBUG mode. This should only be used in development.
    pub allow_debug: bool,
    // TODO(phlip9): new type
    /// The set of trusted enclave measurements. If set to `None`, ignore the
    /// `mrenclave` field.
    pub trusted_mrenclaves: Option<Vec<[u8; 32]>>,
    // TODO(phlip9): new type
    /// The trusted enclave signer key id. If set to `None`, ignore the
    /// `mrsigner` field.
    pub trusted_mrsigner: Option<[u8; 32]>,
}

impl EnclavePolicy {
    /// A policy that trusts any enclave.
    pub fn dangerous_trust_any() -> Self {
        Self {
            allow_debug: true,
            trusted_mrenclaves: None,
            trusted_mrsigner: None,
        }
    }

    /// A policy that trusts the current Intel Quoting Enclave (QE).
    ///
    /// Just checks against an expected QE MRSIGNER for now. In the future this
    /// should take a signed QE identity json from the Intel Trusted Services
    /// API.
    ///
    /// You can get the current QE identity from:
    ///
    /// ```bash
    /// $ curl https://api.trustedservices.intel.com/sgx/certification/v3/qe/identity \
    ///     | jq .enclaveIdentity.mrsigner
    /// ```
    pub fn trust_intel_qe() -> Self {
        const QE_IDENTITY_MRSIGNER: [u8; 32] = hex::decode_const(
            b"8c4f5775d796503e96137f77c68a829a0056ac8ded70140b081b094490c57bff",
        );

        Self {
            allow_debug: false,
            trusted_mrenclaves: None,
            trusted_mrsigner: Some(QE_IDENTITY_MRSIGNER),
        }
    }

    /// A policy that trusts only the local enclave. Useful in tests.
    pub fn trust_self() -> Self {
        #[cfg(target_env = "sgx")]
        {
            let self_report = sgx_isa::Report::for_self();
            let allow_debug = self_report
                .attributes
                .flags
                .contains(sgx_isa::AttributesFlags::DEBUG);
            let trusted_mrenclaves = Some(vec![self_report.mrenclave]);
            let trusted_mrsigner = Some(self_report.mrsigner);
            Self {
                allow_debug,
                trusted_mrenclaves,
                trusted_mrsigner,
            }
        }
        #[cfg(not(target_env = "sgx"))]
        {
            // TODO(phlip9): add some SGX interface that will provide fixed
            // dummy values outside SGX
            Self::dangerous_trust_any()
        }
    }

    /// Verify that an enclave [`sgx_isa::Report`] is trustworthy according to
    /// this policy. Returns the `ReportData` if the verification is successful.
    pub fn verify<'a>(
        &self,
        report: &'a sgx_isa::Report,
    ) -> Result<&'a [u8; 64]> {
        if !self.allow_debug {
            let is_debug = report
                .attributes
                .flags
                .contains(sgx_isa::AttributesFlags::DEBUG);
            ensure!(!is_debug, "enclave is in debug mode",);
        }

        if let Some(mrenclaves) = self.trusted_mrenclaves.as_ref() {
            ensure!(
                mrenclaves.contains(&report.mrenclave),
                "enclave measurement '{}' is not trusted",
                hex::display(&report.mrenclave),
            );
        }

        if let Some(mrsigner) = self.trusted_mrsigner.as_ref() {
            ensure!(
                mrsigner == &report.mrsigner,
                "enclave signer '{}' is not trusted, trusted signer: '{}'",
                hex::display(&report.mrsigner),
                hex::display(mrsigner),
            );
        }

        Ok(&report.reportdata)
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

fn read_attestation_pk(
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

/// The serialized [`Report`] in the Quote has its `keyid` and `mac`
/// fields removed. This function pads the input with `\x00` to match the
/// expected [`Report`] size, then deserializes it.
///
/// [`Report`]: sgx_isa::Report
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

/// A small struct for pretty-printing a [`sgx_isa::Report`].
struct ReportDebug<'a>(&'a sgx_isa::Report);

impl<'a> fmt::Debug for ReportDebug<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Report")
            .field("cpusvn", &hex::display(&self.0.cpusvn))
            .field("miscselect", &self.0.miscselect)
            .field("attributes", &self.0.attributes.flags)
            .field("xfrm", &format!("{:016x}", self.0.attributes.xfrm))
            .field("mrenclave", &hex::display(&self.0.mrenclave))
            .field("mrsigner", &hex::display(&self.0.mrsigner))
            .field("isvprodid", &self.0.isvprodid)
            .field("isvsvn", &self.0.isvsvn)
            .field("reportdata", &hex::display(&self.0.reportdata))
            .field("keyid", &hex::display(&self.0.keyid))
            .field("mac", &hex::display(&self.0.mac))
            .finish()
    }
}

fn rustls_err(s: impl fmt::Display) -> rustls::Error {
    rustls::Error::General(s.to_string())
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

    // TODO(phlip9): test verification catches bad evidence

    fn example_mrenclave() -> [u8; 32] {
        let mut mrenclave = [0u8; 32];
        hex::decode_to_slice(MRENCLAVE_HEX.trim(), mrenclave.as_mut_slice())
            .unwrap();
        mrenclave
    }

    fn mock_timestamp() -> SystemTime {
        // some time in 2022 lol
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(1_660_000_000))
            .unwrap()
    }

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
    fn test_verify_sgx_server_quote() {
        let cert_der = parse_cert_pem_to_der(SGX_SERVER_CERT_PEM).unwrap();
        let evidence = AttestEvidence::parse_cert_der(&cert_der).unwrap();

        let now = SystemTime::now();
        let verifier = SgxQuoteVerifier;
        let report = verifier.verify(&evidence.attest.quote, now).unwrap();

        // println!("{:#?}", ReportDebug(&report));

        let enclave_policy = EnclavePolicy {
            allow_debug: true,
            trusted_mrenclaves: Some(vec![example_mrenclave()]),
            trusted_mrsigner: None,
        };
        enclave_policy.verify(&report).unwrap();
    }

    #[test]
    fn test_verify_sgx_server_cert() {
        let cert_der = parse_cert_pem_to_der(SGX_SERVER_CERT_PEM).unwrap();

        let verifier = ServerCertVerifier {
            expect_dummy_quote: false,
            enclave_policy: EnclavePolicy {
                allow_debug: true,
                trusted_mrenclaves: Some(vec![example_mrenclave()]),
                trusted_mrsigner: None,
            },
        };

        let intermediates = &[];
        let mut scts = iter::empty();
        let ocsp_response = &[];

        verifier
            .verify_server_cert(
                &rustls::Certificate(cert_der),
                intermediates,
                &rustls::ServerName::try_from("localhost").unwrap(),
                &mut scts,
                ocsp_response,
                mock_timestamp(),
            )
            .unwrap();
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
            enclave_policy: EnclavePolicy::dangerous_trust_any(),
        };

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
                mock_timestamp(),
            )
            .unwrap();
    }
}
