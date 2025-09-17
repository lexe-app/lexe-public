//! Verify remote attestation endorsements directly or embedded in x509 certs.

use std::{
    fmt::{self, Debug, Display},
    include_bytes,
    io::Cursor,
    sync::{Arc, LazyLock},
};

use anyhow::{bail, ensure, format_err, Context};
use asn1_rs::FromDer;
use byte_array::ByteArray;
use common::{
    ed25519,
    enclave::{self, Measurement},
    env::DeployEnv,
};
use dcap_ql::quote::{
    Qe3CertDataPckCertChain, Quote, Quote3SignatureEcdsaP256,
};
use rustls::{
    client::danger::{
        HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    DigitallySignedStruct, DistinguishedName,
};
use webpki::{TlsServerTrustAnchors, TrustAnchor};
use x509_parser::certificate::X509Certificate;

use super::quote::ReportData;
use crate::attestation::cert::SgxAttestationExtension;

/// The Enclave Signer Measurement (MRSIGNER) of the current Intel Quoting
/// Enclave (QE).
const INTEL_QE_IDENTITY_MRSIGNER: Measurement =
    Measurement::new(hex::decode_const(
        b"8c4f5775d796503e96137f77c68a829a0056ac8ded70140b081b094490c57bff",
    ));

/// The DER-encoded Intel SGX trust anchor cert.
const INTEL_SGX_ROOT_CA_CERT_DER: &[u8] =
    include_bytes!("../../data/intel-sgx-root-ca.der");

/// Lazily parse the Intel SGX trust anchor cert.
///
/// NOTE: It's easier to inline the cert DER bytes vs. PEM file, otherwise the
/// `TrustAnchor` tries to borrow from a temporary `Vec<u8>`.
static INTEL_SGX_TRUST_ANCHOR: LazyLock<[TrustAnchor<'static>; 1]> =
    LazyLock::new(|| {
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

/// A [`ClientCertVerifier`] and [`ServerCertVerifier`] which verifies embedded
/// remote attestation evidence according to a configured [`EnclavePolicy`].
///
/// Clients or servers connecting to a remote enclave use this to check that:
/// (1) the remote's certificate is valid,
/// (2) the remote attestation is valid (according to the enclave policy), and
/// (3) the remote attestation binds to the presented certificate key pair. Once
/// these checks are successful, the client and server (one of which is inside
/// an SGX enclave) can establish a secure TLS channel.
#[derive(Debug)]
pub struct AttestationCertVerifier {
    /// if `true`, expect a fake dummy quote. Used only for testing.
    pub expect_dummy_quote: bool,
    /// the verifier's policy for trusting the remote enclave.
    pub enclave_policy: EnclavePolicy,
}

/// Whether the verifier is currently being used to verify client or server
/// certs, along with any additional parameters if necessary.
enum VerifierParams<'param> {
    /// Server cert verification params.
    Server {
        server_name: &'param ServerName<'param>,
        ocsp_response: &'param [u8],
    },
    /// Client cert verification params.
    Client,
}

/// A client or server cert verification token.
enum CertVerified {
    Client(ClientCertVerified),
    Server(ServerCertVerified),
}

impl AttestationCertVerifier {
    /// Shared logic for verifying a client or server attestation cert.
    fn verify_attestation_cert(
        &self,
        verifier_params: VerifierParams,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        now: UnixTime,
    ) -> Result<CertVerified, rustls::Error> {
        // there should be no intermediate certs
        if !intermediates.is_empty() {
            return Err(rustls_err("received unexpected intermediate certs"));
        }

        // 1. verify the self-signed cert "normally"; ensure everything is
        //    well-formed, signatures verify, validity ranges OK, SNI matches,
        //    etc...
        let mut trust_roots = rustls::RootCertStore::empty();
        trust_roots.add(end_entity.to_owned()).map_err(rustls_err)?;
        let cert_verified = match verifier_params {
            VerifierParams::Client => {
                let webpki_verifier =
                    rustls::server::WebPkiClientVerifier::builder_with_provider(
                        Arc::new(trust_roots),
                        crate::LEXE_CRYPTO_PROVIDER.clone(),
                    )
                    .build()
                    .map_err(|e| rustls::Error::General(e.to_string()))?;
                webpki_verifier
                    .verify_client_cert(end_entity, &[], now)
                    .map(CertVerified::Client)?
            }
            VerifierParams::Server {
                server_name,
                ocsp_response,
            } => {
                let webpki_verifier =
                    rustls::client::WebPkiServerVerifier::builder_with_provider(
                        Arc::new(trust_roots),
                        crate::LEXE_CRYPTO_PROVIDER.clone(),
                    )
                    .build()
                    .map_err(|e| rustls::Error::General(e.to_string()))?;
                webpki_verifier
                    .verify_server_cert(
                        end_entity,
                        &[],
                        server_name,
                        ocsp_response,
                        now,
                    )
                    .map(CertVerified::Server)?
            }
        };

        // 2. extract enclave attestation quote from the cert
        let evidence = AttestEvidence::parse_cert_der(end_entity)?;

        // 3. verify Quote
        let enclave_report = if self.expect_dummy_quote {
            sgx_isa::Report::try_copy_from(evidence.cert_ext.quote.as_ref())
                .ok_or_else(|| rustls_err("Could not copy Report"))?
        } else {
            let quote_verifier = SgxQuoteVerifier;
            quote_verifier
                .verify(&evidence.cert_ext.quote, now)
                .map_err(|err| {
                    rustls_err(format!("invalid SGX Quote: {err:#}"))
                })?
        };

        // 4. check that this enclave satisfies our enclave policy
        let reportdata =
            self.enclave_policy.verify(&enclave_report).map_err(|err| {
                rustls_err(format!(
                    "our trust policy rejected the remote enclave: {err:#}"
                ))
            })?;

        // 5. check that the pk in the enclave Report matches the one in this
        //    x509 cert.
        if !reportdata.contains(&evidence.cert_pk) {
            return Err(rustls_err(
                "enclave's report is not binding to the presented x509 cert",
            ));
        }

        Ok(cert_verified)
    }
}

impl ServerCertVerifier for AttestationCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        server_name: &ServerName,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verifier_params = VerifierParams::Server {
            server_name,
            ocsp_response,
        };
        let cert_verified = self.verify_attestation_cert(
            verifier_params,
            end_entity,
            intermediates,
            now,
        )?;

        match cert_verified {
            CertVerified::Client(_) =>
                panic!("verify_attestation_cert returned wrong token kind"),
            CertVerified::Server(verified) => Ok(verified),
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
            &crate::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        crate::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
    }
}

impl ClientCertVerifier for AttestationCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer,
        intermediates: &[CertificateDer],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        let verifier_params = VerifierParams::Client;
        let cert_verified = self.verify_attestation_cert(
            verifier_params,
            end_entity,
            intermediates,
            now,
        )?;

        match cert_verified {
            CertVerified::Client(verified) => Ok(verified),
            CertVerified::Server(_) =>
                panic!("verify_attestation_cert returned wrong token kind"),
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
            &crate::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        crate::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
    }
}

/// TODO(max): Needs docs
pub struct AttestEvidence<'quote> {
    cert_pk: ed25519::PublicKey,
    cert_ext: SgxAttestationExtension<'quote>,
}

impl<'a> AttestEvidence<'a> {
    pub fn parse_cert_der(cert_der: &'a [u8]) -> Result<Self, rustls::Error> {
        use std::io;

        /// Shorthand to construct a [`rustls::Error::InvalidCertificate`]
        /// from a [`std::error::Error`] impl.
        fn invalid_cert_error(
            error: impl std::error::Error + Send + Sync + 'static,
        ) -> rustls::Error {
            let other_error = rustls::OtherError(Arc::new(error));
            let cert_error = rustls::CertificateError::Other(other_error);
            rustls::Error::InvalidCertificate(cert_error)
        }

        // TODO(phlip9): manually parse the cert fields we care about w/ yasna
        // instead of pulling in a whole extra x509 cert parser...

        let (unparsed_data, cert) =
            X509Certificate::from_der(cert_der).map_err(invalid_cert_error)?;

        if !unparsed_data.is_empty() {
            // Need to construct something that impls `std::error::Error`
            // Apparently `anyhow::Error` doesn't implement `std::error::Error`?
            let msg = "leftover unparsed cert data";
            let io_error = io::Error::other(msg);
            return Err(invalid_cert_error(io_error));
        }

        let cert_pk = ed25519::PublicKey::try_from(cert.public_key())
            .map_err(invalid_cert_error)?;

        let sgx_ext_oid = SgxAttestationExtension::oid_asn1_rs();
        let cert_ext = cert
            .get_extension_unique(&sgx_ext_oid)
            .map_err(invalid_cert_error)?
            .ok_or_else(|| {
                let msg = "no SGX attestation extension";
                invalid_cert_error(io::Error::other(msg))
            })?;

        let cert_ext = SgxAttestationExtension::from_der_bytes(cert_ext.value)
            .map_err(|e| {
                let msg = format!("invalid SGX attestation: {e:#}");
                invalid_cert_error(io::Error::other(msg))
            })?;

        Ok(Self { cert_pk, cert_ext })
    }
}

/// A verifier for validating SGX [`Quote`]s.
///
/// To read a more in-depth explanation of the verification logic and see some
/// pretty pictures showing the chain of trust from the Intel SGX root CA down
/// to the application enclave's ReportData, visit:
/// [phlip9.com/notes - SGX Remote Attestation Quote Verification](https://phlip9.com/notes/confidential%20computing/intel%20SGX/remote%20attestation/#remote-attestation-quote-verification)
pub struct SgxQuoteVerifier;

impl SgxQuoteVerifier {
    /// TODO(max): Needs docs - esp wrt the report returned here
    pub fn verify(
        &self,
        quote_bytes: &[u8],
        now: UnixTime,
    ) -> anyhow::Result<sgx_isa::Report> {
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

        let now = webpki::Time::from_seconds_since_unix_epoch(now.as_secs());

        // We don't use the full `rustls::WebPkiServerVerifier` here since the
        // PCK certs aren't truly webpki certs, as they don't bind to DNS names.
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
            &qe3_reportdata.as_inner()[..32] == expected_reportdata.as_slice(),
            "Quoting Enclave's Report data doesn't match the Quote attestation pk: \
             actual: '{}', expected: '{}'",
            hex::display(&qe3_reportdata.as_inner()[..32]),
            expected_reportdata,
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
    fn new(err: impl Display) -> Self {
        Self(format!("{err:#}"))
    }
}

impl Display for DisplayErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DisplayErr {
    fn description(&self) -> &str {
        &self.0
    }
}

fn parse_cert_pem_to_der(s: &str) -> anyhow::Result<Vec<u8>> {
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
#[derive(Debug)]
pub struct EnclavePolicy {
    /// Allow enclaves in DEBUG mode. This should only be used in development.
    pub allow_debug: bool,
    /// The set of trusted enclave measurements. If set to `None`, ignore the
    /// `mrenclave` field.
    pub trusted_mrenclaves: Option<Vec<Measurement>>,
    /// The trusted enclave signer key id. If set to `None`, ignore the
    /// `mrsigner` field.
    pub trusted_mrsigner: Option<Measurement>,
}

impl EnclavePolicy {
    /// An [`EnclavePolicy`] which only trusts the given [`Measurement`]s, and
    /// which must be signed by an appropriate signer, taking into account our
    /// deploy environment and whether we're actually expecting an SGX enclave.
    /// This is generally what you want.
    pub fn trust_measurements_with_signer(
        use_sgx: bool,
        deploy_env: DeployEnv,
        measurements: Vec<Measurement>,
    ) -> Self {
        Self {
            allow_debug: deploy_env.is_dev(),
            trusted_mrenclaves: Some(measurements),
            trusted_mrsigner: Some(Measurement::expected_signer(
                use_sgx, deploy_env,
            )),
        }
    }

    /// An [`EnclavePolicy`] which trusts any measurement signed by the
    /// [`Measurement::expected_signer`].
    pub fn trust_expected_signer(use_sgx: bool, deploy_env: DeployEnv) -> Self {
        Self {
            allow_debug: deploy_env.is_dev(),
            trusted_mrenclaves: None,
            trusted_mrsigner: Some(Measurement::expected_signer(
                use_sgx, deploy_env,
            )),
        }
    }

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
        Self {
            allow_debug: false,
            trusted_mrenclaves: None,
            trusted_mrsigner: Some(INTEL_QE_IDENTITY_MRSIGNER),
        }
    }

    /// A policy that trusts only the local enclave. Useful in tests.
    pub fn trust_self() -> Self {
        let self_report = enclave::report();
        let report_mrenclave = Measurement::new(self_report.mrenclave);
        let report_mrsigner = Measurement::new(self_report.mrsigner);

        let allow_debug = self_report
            .attributes
            .flags
            .contains(sgx_isa::AttributesFlags::DEBUG);
        let trusted_mrenclaves = Some(vec![report_mrenclave]);
        let trusted_mrsigner = Some(report_mrsigner);

        Self {
            allow_debug,
            trusted_mrenclaves,
            trusted_mrsigner,
        }
    }

    /// Verify that an enclave [`sgx_isa::Report`] is trustworthy according to
    /// this policy. Returns the [`ReportData`] if verification is successful.
    pub fn verify(
        &self,
        report: &sgx_isa::Report,
    ) -> anyhow::Result<ReportData> {
        if !self.allow_debug {
            let is_debug = report
                .attributes
                .flags
                .contains(sgx_isa::AttributesFlags::DEBUG);
            ensure!(!is_debug, "enclave is in debug mode",);
        }

        let report_mrenclave = Measurement::new(report.mrenclave);
        if let Some(mrenclaves) = self.trusted_mrenclaves.as_ref() {
            ensure!(
                mrenclaves.contains(&report_mrenclave),
                "enclave measurement '{report_mrenclave}' is not trusted",
            );
        }

        let report_mrsigner = Measurement::new(report.mrsigner);
        if let Some(mrsigner) = self.trusted_mrsigner.as_ref() {
            ensure!(
                mrsigner == &report_mrsigner,
                "enclave signer '{report_mrsigner}' is not trusted, trusted signer: '{mrsigner}'",
            );
        }

        Ok(ReportData::new(report.reportdata))
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
fn get_ecdsa_sig_der(sig: &[u8]) -> anyhow::Result<Vec<u8>> {
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
) -> anyhow::Result<ring::signature::UnparsedPublicKey<[u8; 65]>> {
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
fn report_try_from_truncated(bytes: &[u8]) -> anyhow::Result<sgx_isa::Report> {
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
pub struct ReportDebug<'a>(&'a sgx_isa::Report);

impl Debug for ReportDebug<'_> {
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

/// Convenience to create a [`rustls::Error`] from a [`Display`]able object.
fn rustls_err(s: impl Display) -> rustls::Error {
    rustls::Error::General(s.to_string())
}

#[cfg(test)]
mod test {
    use std::{include_str, time::Duration};

    use super::*;

    // These two consts can be regenerated (in SGX) using [`dump_attest_cert`].
    const SGX_SERVER_CERT_PEM: &str =
        include_str!("../../test_data/attest_cert.pem");
    const SERVER_MRENCLAVE: Measurement = Measurement::new(hex::decode_const(
        b"738f61792535f905807365a0f6023275b6a44972f48986c94aa7976c31bf1eb6",
    ));

    const INTEL_SGX_ROOT_CA_CERT_PEM: &str =
        include_str!("../../test_data/intel-sgx-root-ca.pem");

    // TODO(phlip9): test verification catches bad evidence

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

        let now = UnixTime::now();
        let verifier = SgxQuoteVerifier;
        let report = verifier.verify(&evidence.cert_ext.quote, now).unwrap();

        // println!("{:#?}", ReportDebug(&report));

        let enclave_policy = EnclavePolicy {
            allow_debug: true,
            trusted_mrenclaves: Some(vec![SERVER_MRENCLAVE]),
            trusted_mrsigner: None,
        };
        enclave_policy.verify(&report).unwrap();
    }

    #[test]
    fn test_verify_sgx_server_cert() {
        let cert_der = parse_cert_pem_to_der(SGX_SERVER_CERT_PEM).unwrap();

        let verifier = AttestationCertVerifier {
            expect_dummy_quote: false,
            enclave_policy: EnclavePolicy {
                allow_debug: true,
                trusted_mrenclaves: Some(vec![SERVER_MRENCLAVE]),
                trusted_mrsigner: None,
            },
        };

        let intermediates = &[];
        let ocsp_response = &[];

        verifier
            .verify_server_cert(
                &CertificateDer::from(cert_der),
                intermediates,
                &ServerName::try_from("localhost").unwrap(),
                ocsp_response,
                UnixTime::now(),
            )
            .unwrap();
    }

    // SGX generates a real quote
    #[cfg(not(target_env = "sgx"))]
    #[test]
    fn test_verify_dummy_server_cert() {
        use common::rng::FastRng;

        use crate::attestation::cert::AttestationCert;

        let mut rng = FastRng::new();
        let dns_name = "run.lexe.app".to_owned();
        let lifetime = Duration::from_secs(60);

        let cert = AttestationCert::generate(&mut rng, &[&dns_name], lifetime)
            .unwrap();
        let cert_der = cert.serialize_der_self_signed().unwrap();

        let verifier = AttestationCertVerifier {
            expect_dummy_quote: true,
            enclave_policy: EnclavePolicy::dangerous_trust_any(),
        };

        let intermediates = &[];
        let ocsp_response = &[];

        verifier
            .verify_server_cert(
                &cert_der.into(),
                intermediates,
                &ServerName::try_from(dns_name.as_str()).unwrap(),
                ocsp_response,
                UnixTime::now(),
            )
            .unwrap();
    }

    /// Dump fresh attestation cert (intended for SGX only):
    ///
    /// ```bash
    /// cargo test -p common --target=x86_64-fortanix-unknown-sgx dump_attest_cert -- --ignored --show-output
    /// ```
    #[test]
    #[cfg(target_env = "sgx")]
    #[ignore]
    fn dump_attest_cert() {
        use base64::Engine;

        use crate::{
            attestation::cert::AttestationCert, enclave, rng::FastRng,
        };

        let mut rng = FastRng::new();
        let dns_name = "localhost".to_owned();
        // Use a long lifetime so the test won't fail just bc the cert expired
        let lifetime = Duration::from_secs(60 * 60 * 24 * 365 * 1000);

        let attest_cert =
            AttestationCert::generate(&mut rng, &[&dns_name], lifetime)
                .unwrap();

        println!("measurement: '{}'", enclave::measurement());
        println!("Set `SERVER_MRENCLAVE` to this value.");

        let cert_der = attest_cert.serialize_der_self_signed().unwrap();
        let cert_base64 = base64::engine::general_purpose::STANDARD
            .encode(cert_der.as_slice());

        println!("attestation certificate:");
        println!("-----BEGIN CERTIFICATE-----");
        println!("{cert_base64}");
        println!("-----END CERTIFICATE-----");
    }
}
