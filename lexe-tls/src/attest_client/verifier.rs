//! Verify remote attestation endorsements directly or embedded in x509 certs.

use std::{
    fmt::{self, Debug, Display},
    include_bytes,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, bail, ensure, format_err};
use asn1_rs::FromDer;
use dcap_ql::quote::{
    CertificationDataType, Quote, Quote3SignatureEcdsaP256, RawQe3CertData,
};
use lexe_byte_array::ByteArray;
use lexe_common::env::DeployEnv;
use lexe_crypto::ed25519;
use lexe_enclave::enclave::{self, Measurement};
use lexe_hex::hex;
use lexe_sha256::sha256;
use rustls::{
    DigitallySignedStruct, DistinguishedName,
    client::danger::{
        HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
    },
    pki_types::{
        CertificateDer, ServerName, SignatureVerificationAlgorithm,
        TrustAnchor, UnixTime, pem::PemObject,
    },
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use x509_parser::certificate::X509Certificate;

use crate::{
    attest_client::{
        cert::{CpuFmspc, SgxAttestationExtension, SgxPckExtensions},
        quote::ReportData,
    },
    ed25519_ext::Ed25519PublicKeyExt,
};

/// The Enclave Signer Measurement (MRSIGNER) of the current Intel Quoting
/// Enclave (QE).
const INTEL_QE_IDENTITY_MRSIGNER: Measurement =
    Measurement::new(hex::decode_const(
        b"8c4f5775d796503e96137f77c68a829a0056ac8ded70140b081b094490c57bff",
    ));

/// The DER-encoded Intel SGX trust anchor cert.
const INTEL_SGX_ROOT_CA_CERT_DER: &CertificateDer<'static> =
    &CertificateDer::from_slice(include_bytes!(
        "../../data/intel-sgx-root-ca.der"
    ));

/// Lazily parse the Intel SGX trust anchor cert.
///
/// NOTE: It's easier to inline the cert DER bytes vs. PEM file, otherwise the
/// `TrustAnchor` tries to borrow from a temporary `Vec<u8>`.
static INTEL_SGX_TRUST_ANCHOR: LazyLock<[TrustAnchor<'static>; 1]> =
    LazyLock::new(|| {
        let trust_anchor = webpki::anchor_from_trusted_cert(
            INTEL_SGX_ROOT_CA_CERT_DER,
        )
        .expect("Failed to deserialize Intel SGX root CA cert from der bytes");

        [trust_anchor]
    });

/// Supported signature schemes for verifying SGX quote chain certs.
///
/// Since we don't control the Intel certs used in the SGX quote chain, we'll
/// need to support a wider range of signature algorithms. We will however
/// remove legacy algorithms.
static SUPPORTED_SIG_ALGS: &[&dyn SignatureVerificationAlgorithm] = &[
    webpki::ring::ECDSA_P256_SHA256,
    webpki::ring::ECDSA_P256_SHA384,
    webpki::ring::ECDSA_P384_SHA256,
    webpki::ring::ECDSA_P384_SHA384,
    webpki::ring::ED25519,
    webpki::ring::RSA_PKCS1_2048_8192_SHA256,
    webpki::ring::RSA_PKCS1_2048_8192_SHA384,
    webpki::ring::RSA_PKCS1_2048_8192_SHA512,
    webpki::ring::RSA_PKCS1_3072_8192_SHA384,
];

/// Reject remote attestations from these Intel CPUs
///
/// These CPUs are missing hardware/microcode mitigations that require extreme
/// performance penalties to mitigate in software. This also allows us to
/// easily ban old CPUs as new vulnerabilities are discovered.
///
/// Current block criteria:
/// - CPU generations older than Ice Lake
/// - Consumer desktop/mobile CPUs
///
/// We only need to specify CPUs that support SGX and that Intel PCCS still
/// allows for remote attestation.
#[rustfmt::skip]
const CPU_FMSPC_BLOCKLIST: [CpuFmspc; 14] = [
    // 06_7AH CPUID.01H:EAX[19:0]=0x706A1 F/M/S/eM/P=6/A/1/7/0 FMSPC=00706a100000 INTEL_ATOM_GOLDMONT_PLUS (Gemini Lake, s:1)      platform=client
    CpuFmspc([0x00, 0x70, 0x6a, 0x10, 0x00, 0x00]),
    // 06_7AH CPUID.01H:EAX[19:0]=0x706A8 F/M/S/eM/P=6/A/8/7/0 FMSPC=00706a800000 INTEL_ATOM_GOLDMONT_PLUS (Gemini Lake, s:8)      platform=client
    CpuFmspc([0x00, 0x70, 0x6a, 0x80, 0x00, 0x00]),
    // 06_7EH CPUID.01H:EAX[19:0]=0x706E4 F/M/S/eM/P=6/E/4/7/0 FMSPC=00706e470000 INTEL_ICELAKE_L (Sunny Cove, s:4)                platform=client
    CpuFmspc([0x00, 0x70, 0x6e, 0x47, 0x00, 0x00]),
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EA F/M/S/eM/P=6/E/A/8/0 FMSPC=00806ea60000 INTEL_KABYLAKE_L / COFFEELAKE_L (Sky Lake, s:A)  platform=client
    CpuFmspc([0x00, 0x80, 0x6e, 0xa6, 0x00, 0x00]),
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EB F/M/S/eM/P=6/E/B/8/0 FMSPC=00806eb70000 INTEL_KABYLAKE_L / WHISKEYLAKE_L (Sky Lake, s:B) platform=client
    CpuFmspc([0x00, 0x80, 0x6e, 0xb7, 0x00, 0x00]),
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EB F/M/S/eM/P=6/E/B/8/0 FMSPC=20806eb70000 INTEL_KABYLAKE_L / WHISKEYLAKE_L (Sky Lake, s:B) platform=client
    CpuFmspc([0x20, 0x80, 0x6e, 0xb7, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EA F/M/S/eM/P=6/E/A/9/0 FMSPC=00906ea10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:A)      platform=E3
    CpuFmspc([0x00, 0x90, 0x6e, 0xa1, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EA F/M/S/eM/P=6/E/A/9/0 FMSPC=00906ea50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:A)      platform=client
    CpuFmspc([0x00, 0x90, 0x6e, 0xa5, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EB F/M/S/eM/P=6/E/B/9/0 FMSPC=00906eb10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:B)      platform=client
    CpuFmspc([0x00, 0x90, 0x6e, 0xb1, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=00906ec10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    CpuFmspc([0x00, 0x90, 0x6e, 0xc1, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=00906ec50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    CpuFmspc([0x00, 0x90, 0x6e, 0xc5, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=20906ec10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    CpuFmspc([0x20, 0x90, 0x6e, 0xc1, 0x00, 0x00]),
    // 06_9EH CPUID.01H:EAX[19:0]=0x906ED F/M/S/eM/P=6/E/D/9/0 FMSPC=00906ed50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:D)      platform=E3
    CpuFmspc([0x00, 0x90, 0x6e, 0xd5, 0x00, 0x00]),
    // 06_A5H CPUID.01H:EAX[19:0]=0xA0655 F/M/S/eM/P=6/5/5/A/0 FMSPC=00a065510000 INTEL_COMETLAKE (Sky Lake, s:5)                  platform=client
    CpuFmspc([0x00, 0xa0, 0x65, 0x51, 0x00, 0x00]),
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
                        lexe_tls_core::LEXE_CRYPTO_PROVIDER.clone(),
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
                        lexe_tls_core::LEXE_CRYPTO_PROVIDER.clone(),
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
            &lexe_tls_core::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        lexe_tls_core::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
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
            &lexe_tls_core::LEXE_SIGNATURE_ALGORITHMS,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        lexe_tls_core::LEXE_SUPPORTED_VERIFY_SCHEMES.clone()
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

        let cert_pk = ed25519::PublicKey::try_from_spki(cert.public_key())
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

        ensure!(
            sig.certification_data_type()
                == CertificationDataType::PckCertificateChain,
            "unexpected SGX quote certification data type",
        );

        let cert_chain_pem = sig
            .certification_data::<RawQe3CertData>()
            .map_err(DisplayErr::new)
            .context("Failed to parse PCK cert chain")?;

        // 1. Verify the local SGX platform PCK cert is endorsed by the Intel
        //    SGX trust root CA.

        // [0] : PCK cert
        // [1] : PCK platform cert
        // [2] : SGX root CA cert
        let mut cert_iter = CertificateDer::pem_slice_iter(&cert_chain_pem);
        let pck_cert_der = cert_iter.next().context("Missing PCK cert")??;
        let pck_platform_cert_der =
            cert_iter.next().context("Missing PCK platform cert")??;
        let sgx_root_ca_cert_der =
            cert_iter.next().context("Missing SGX root CA cert")??;
        ensure!(cert_iter.next().is_none(), "unexpected extra certificate");

        // Extract and decode the SGX PCK Extensions from the PCK leaf cert.
        let pck_exts = SgxPckExtensions::from_cert_der(&pck_cert_der)
            .context("Failed to parse SGX PCK cert extensions")?;
        // Reject Intel CPUs that are too old.
        let cpu_fmspc = pck_exts.cpu_fmspc;
        ensure!(
            !CPU_FMSPC_BLOCKLIST.contains(&pck_exts.cpu_fmspc),
            "remote Intel CPU is too old: CPU FMSPC={cpu_fmspc}",
        );

        let pck_cert = webpki::EndEntityCert::try_from(&pck_cert_der)
            .context("Invalid PCK cert")?;

        // We don't use the full `rustls::WebPkiServerVerifier` here since the
        // PCK certs aren't truly webpki certs, as they don't bind to DNS names.
        //
        // Instead, we skip the DNS checks by using the lower-level `EndEntity`
        // verification methods directly.

        let key_usage = webpki::KeyUsage::server_auth();
        let revocation = None;
        let verify_path = None;
        pck_cert
            .verify_for_usage(
                SUPPORTED_SIG_ALGS,
                INTEL_SGX_TRUST_ANCHOR.as_slice(),
                &[pck_platform_cert_der, sgx_root_ca_cert_der],
                now,
                key_usage,
                revocation,
                verify_path,
            )
            .context("PCK cert chain failed validation")?;

        let qe3_sig = get_ecdsa_sig_der(sig.qe3_signature())?;
        let qe3_report_bytes = sig.qe3_report();

        // 2. Verify the Platform Certification Enclave (PCE) endorses the
        //    Quoting Enclave (QE) Report.

        pck_cert
            .verify_signature(
                webpki::ring::ECDSA_P256_SHA256,
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
        let is_dev = deploy_env.is_dev();
        Self {
            allow_debug: is_dev,
            trusted_mrenclaves: Some(measurements),
            trusted_mrsigner: Some(Measurement::expected_signer(
                use_sgx, is_dev,
            )),
        }
    }

    /// An [`EnclavePolicy`] which trusts any measurement signed by the
    /// [`Measurement::expected_signer`].
    pub fn trust_expected_signer(use_sgx: bool, deploy_env: DeployEnv) -> Self {
        let is_dev = deploy_env.is_dev();
        Self {
            allow_debug: is_dev,
            trusted_mrenclaves: None,
            trusted_mrsigner: Some(Measurement::expected_signer(
                use_sgx, is_dev,
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
    if !sig.len().is_multiple_of(2) {
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
    ensure!(
        bytes.len() == 64,
        "Attestation public key is in an unrecognized format; expected exactly 64 bytes, actual len: {}",
        bytes.len()
    );

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
    use std::{fmt, fs, time::Duration};

    use super::*;
    use crate::attest_client::cert::CpuFmspc;

    // TODO(phlip9): test verification catches bad evidence

    // This can be regenerated (in SGX) using
    // `lexe_tls_attest_server::cert::test::dump_attest_cert`.
    fn attest_cert_fixture() -> (CertificateDer<'static>, Measurement) {
        let measurement = Measurement::from_hex(
            "f6331736910c50e065c4b2692932c5ed1b08f779607e2729a06eb35b9ab4b0bf",
        )
        .unwrap();

        let cert_pem = fs::read_to_string("test_data/attest_cert.pem").unwrap();
        let cert_der =
            CertificateDer::from_pem_slice(cert_pem.as_bytes()).unwrap();

        (cert_der, measurement)
    }

    #[test]
    fn test_intel_sgx_trust_anchor_der_pem_equal() {
        let intel_sgx_root_ca_cert_der1 = INTEL_SGX_ROOT_CA_CERT_DER;

        let intel_sgx_root_ca_cert_pem =
            fs::read_to_string("test_data/intel-sgx-root-ca.pem").unwrap();
        let intel_sgx_root_ca_cert_der2 = CertificateDer::from_pem_slice(
            intel_sgx_root_ca_cert_pem.as_bytes(),
        )
        .unwrap();
        assert_eq!(intel_sgx_root_ca_cert_der1, &intel_sgx_root_ca_cert_der2);

        // this should not panic
        let _sgx_trust_anchor = &*INTEL_SGX_TRUST_ANCHOR;
    }

    #[test]
    fn test_verify_sgx_server_quote() {
        let (cert_der, measurement) = attest_cert_fixture();
        let evidence = AttestEvidence::parse_cert_der(&cert_der).unwrap();

        let now = UnixTime::since_unix_epoch(Duration::from_secs(1779307188));
        let verifier = SgxQuoteVerifier;
        let report = verifier.verify(&evidence.cert_ext.quote, now).unwrap();

        // println!("{:#?}", ReportDebug(&report));

        let enclave_policy = EnclavePolicy {
            allow_debug: true,
            trusted_mrenclaves: Some(vec![measurement]),
            trusted_mrsigner: None,
        };
        enclave_policy.verify(&report).unwrap();
    }

    #[test]
    fn test_verify_sgx_server_cert() {
        let (cert_der, measurement) = attest_cert_fixture();

        let verifier = AttestationCertVerifier {
            expect_dummy_quote: false,
            enclave_policy: EnclavePolicy {
                allow_debug: true,
                trusted_mrenclaves: Some(vec![measurement]),
                trusted_mrsigner: None,
            },
        };

        let intermediates = &[];
        let ocsp_response = &[];

        verifier
            .verify_server_cert(
                &cert_der,
                intermediates,
                &ServerName::try_from("localhost").unwrap(),
                ocsp_response,
                UnixTime::now(),
            )
            .unwrap();
    }

    // ```bash
    // $ cargo test -p lexe-tls --lib -- pretty_print_cpu_fmspc --nocapture --ignored
    // ```
    #[test]
    #[ignore]
    fn pretty_print_cpu_fmspc() {
        // Azure DCsv3 Intel Xeon-v3 CPU (Ice Lake)
        let fmspc = CpuFmspc::from_hex("00606a000000").unwrap();
        println!("{}", FmspcCpuid::new(fmspc));
    }

    // ```bash
    // $ cargo test -p lexe-tls --lib -- dump_cpu_fmspc_blocklist --nocapture --ignored
    // ```
    //
    // Update the list of FMSPC values for SGX and TDX platforms supporting DCAP
    // attestation:
    //
    // ```bash
    // $ curl -sSL 'https://api.trustedservices.intel.com/sgx/certification/v4/fmspcs' \
    //     | jq . > public/lexe-tls/test_data/intel-sgx-v4-fmspcs.json
    // ```
    //
    // Dump (2026-05-21):
    //
    // ```
    // ### All CPUs that Intel supports for remote attestation
    // 06_6AH CPUID.01H:EAX[19:0]=0x606A0 F/M/S/eM/P=6/A/0/6/0 FMSPC=00606a000000 INTEL_ICELAKE_X (Sunny Cove, s:0)                platform=E5
    // 06_6AH CPUID.01H:EAX[19:0]=0x606A0 F/M/S/eM/P=6/A/0/6/0 FMSPC=30606a000000 INTEL_ICELAKE_X (Sunny Cove, s:0)                platform=E5
    // 06_6CH CPUID.01H:EAX[19:0]=0x606C0 F/M/S/eM/P=6/C/0/6/0 FMSPC=00606c040000 INTEL_ICELAKE_D (Sunny Cove, s:0)                platform=E5
    // 06_6CH CPUID.01H:EAX[19:0]=0x606C0 F/M/S/eM/P=6/C/0/6/0 FMSPC=20606c040000 INTEL_ICELAKE_D (Sunny Cove, s:0)                platform=E5
    // 06_7AH CPUID.01H:EAX[19:0]=0x706A1 F/M/S/eM/P=6/A/1/7/0 FMSPC=00706a100000 INTEL_ATOM_GOLDMONT_PLUS (Gemini Lake, s:1)      platform=client
    // 06_7AH CPUID.01H:EAX[19:0]=0x706A8 F/M/S/eM/P=6/A/8/7/0 FMSPC=00706a800000 INTEL_ATOM_GOLDMONT_PLUS (Gemini Lake, s:8)      platform=client
    // 06_7EH CPUID.01H:EAX[19:0]=0x706E4 F/M/S/eM/P=6/E/4/7/0 FMSPC=00706e470000 INTEL_ICELAKE_L (Sunny Cove, s:4)                platform=client
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EA F/M/S/eM/P=6/E/A/8/0 FMSPC=00806ea60000 INTEL_KABYLAKE_L / COFFEELAKE_L (Sky Lake, s:A)  platform=client
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EB F/M/S/eM/P=6/E/B/8/0 FMSPC=00806eb70000 INTEL_KABYLAKE_L / WHISKEYLAKE_L (Sky Lake, s:B) platform=client
    // 06_8EH CPUID.01H:EAX[19:0]=0x806EB F/M/S/eM/P=6/E/B/8/0 FMSPC=20806eb70000 INTEL_KABYLAKE_L / WHISKEYLAKE_L (Sky Lake, s:B) platform=client
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=00806f000000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=00806f050000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=30806f040000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=50806f000000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=90806f000000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=c0806f000000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_8FH CPUID.01H:EAX[19:0]=0x806F0 F/M/S/eM/P=6/F/0/8/0 FMSPC=f0806f000000 INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:0)        platform=E5
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EA F/M/S/eM/P=6/E/A/9/0 FMSPC=00906ea10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:A)      platform=E3
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EA F/M/S/eM/P=6/E/A/9/0 FMSPC=00906ea50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:A)      platform=client
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EB F/M/S/eM/P=6/E/B/9/0 FMSPC=00906eb10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:B)      platform=client
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=00906ec10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=00906ec50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    // 06_9EH CPUID.01H:EAX[19:0]=0x906EC F/M/S/eM/P=6/E/C/9/0 FMSPC=20906ec10000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:C)      platform=client
    // 06_9EH CPUID.01H:EAX[19:0]=0x906ED F/M/S/eM/P=6/E/D/9/0 FMSPC=00906ed50000 INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:D)      platform=E3
    // 06_A5H CPUID.01H:EAX[19:0]=0xA0655 F/M/S/eM/P=6/5/5/A/0 FMSPC=00a065510000 INTEL_COMETLAKE (Sky Lake, s:5)                  platform=client
    // 06_A7H CPUID.01H:EAX[19:0]=0xA0671 F/M/S/eM/P=6/7/1/A/0 FMSPC=00a067110000 INTEL_ROCKETLAKE (Cypress Cove, s:1)             platform=E3
    // 06_ADH CPUID.01H:EAX[19:0]=0xA06D0 F/M/S/eM/P=6/D/0/A/0 FMSPC=00a06d080000 INTEL_GRANITERAPIDS_X (s:0)                      platform=E5
    // 06_ADH CPUID.01H:EAX[19:0]=0xA06D0 F/M/S/eM/P=6/D/0/A/0 FMSPC=10a06d000000 INTEL_GRANITERAPIDS_X (s:0)                      platform=E5
    // 06_ADH CPUID.01H:EAX[19:0]=0xA06D0 F/M/S/eM/P=6/D/0/A/0 FMSPC=20a06d080000 INTEL_GRANITERAPIDS_X (s:0)                      platform=E5
    // 06_ADH CPUID.01H:EAX[19:0]=0xA06D0 F/M/S/eM/P=6/D/0/A/0 FMSPC=70a06d070000 INTEL_GRANITERAPIDS_X (s:0)                      platform=E5
    // 06_AEH CPUID.01H:EAX[19:0]=0xA06E0 F/M/S/eM/P=6/E/0/A/0 FMSPC=00a06e050000 INTEL_GRANITERAPIDS_D (s:0)                      platform=E5
    // 06_AEH CPUID.01H:EAX[19:0]=0xA06E0 F/M/S/eM/P=6/E/0/A/0 FMSPC=20a06e050000 INTEL_GRANITERAPIDS_D (s:0)                      platform=E5
    // 06_AFH CPUID.01H:EAX[19:0]=0xA06F0 F/M/S/eM/P=6/F/0/A/0 FMSPC=10a06f010000 INTEL_ATOM_CRESTMONT_X (Sierra Forest, s:0)      platform=E5
    // 06_AFH CPUID.01H:EAX[19:0]=0xA06F0 F/M/S/eM/P=6/F/0/A/0 FMSPC=20a06f000000 INTEL_ATOM_CRESTMONT_X (Sierra Forest, s:0)      platform=E5
    // 06_AFH CPUID.01H:EAX[19:0]=0xA06F0 F/M/S/eM/P=6/F/0/A/0 FMSPC=60a06f000000 INTEL_ATOM_CRESTMONT_X (Sierra Forest, s:0)      platform=E5
    // 06_CFH CPUID.01H:EAX[19:0]=0xC06F0 F/M/S/eM/P=6/F/0/C/0 FMSPC=90c06f000000 INTEL_EMERALDRAPIDS_X (s:0)                      platform=E5
    // 06_CFH CPUID.01H:EAX[19:0]=0xC06F0 F/M/S/eM/P=6/F/0/C/0 FMSPC=b0c06f000000 INTEL_EMERALDRAPIDS_X (s:0)                      platform=E5
    // ```
    #[test]
    #[ignore]
    fn dump_cpu_fmspc_blocklist() {
        let fmspcs_str =
            fs::read_to_string("test_data/intel-sgx-v4-fmspcs.json").unwrap();
        let fmspcs_json: serde_json::Value =
            serde_json::from_str(&fmspcs_str).unwrap();

        let mut entries = fmspcs_json
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let fmspc = entry["fmspc"].as_str().unwrap();
                let fmspc =
                    CpuFmspc::from_hex(&fmspc.to_ascii_lowercase()).unwrap();
                let fmspc_cpuid = FmspcCpuid::new(fmspc);

                let platform = entry["platform"].as_str().unwrap();

                FmspcPrettyPrintEntry {
                    fmspc_cpuid,
                    platform: platform.to_owned(),
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by_cached_key(|entry| entry.to_string());

        println!("### All CPUs that Intel supports for remote attestation");
        for entry in &entries {
            println!("{entry}")
        }

        let blocked_entries = entries
            .iter()
            .filter(|entry| entry.is_blocked())
            .collect::<Vec<_>>();

        println!();
        println!("### Blocked CPUs (older than Ice Lake or platform=client):");
        for entry in &blocked_entries {
            println!("{entry}")
        }

        println!("\n```");
        println!(
            "const CPU_FMSPC_BLOCKLIST: [CpuFmspc; {}] = [",
            blocked_entries.len()
        );
        for entry in blocked_entries {
            let [a, b, c, d, e, f] = entry.fmspc_cpuid.fmspc.0;
            let fmspc = format!(
                "CpuFmspc([0x{a:02x}, 0x{b:02x}, 0x{c:02x}, 0x{d:02x}, 0x{e:02x}, 0x{f:02x}])"
            );

            println!("    // {entry}");
            println!("    {fmspc},");
        }
        println!("];");
        println!("```");
    }

    /// A decoded FMSPC code and the Intel "platform" kind (client, E3, E5).
    /// `client` platforms are consumer desktop/mobile CPUs.
    struct FmspcPrettyPrintEntry {
        fmspc_cpuid: FmspcCpuid,
        platform: String,
    }

    impl FmspcPrettyPrintEntry {
        fn is_blocked(&self) -> bool {
            self.fmspc_cpuid.is_older_than_icelake()
                || self.platform == "client"
        }
    }

    impl fmt::Display for FmspcPrettyPrintEntry {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self {
                fmspc_cpuid,
                platform,
            } = self;
            write!(f, "{fmspc_cpuid} platform={platform}")
        }
    }

    /// A decoded FMSPC code
    struct FmspcCpuid {
        fmspc: CpuFmspc,
        cpuid_01_eax_low20: u32,
        family_id: u8,
        model_id: u8,
        stepping_id: u8,
        extended_model_id: u8,
        processor_type: u8,
        display_family: u8,
        display_model: u8,
    }

    impl FmspcCpuid {
        fn new(fmspc: CpuFmspc) -> Self {
            // FMSPC is 6 bytes. For current Intel SGX PCS FMSPC values, bytes
            // 1, 2, and the high nibble of byte 3 encode the low 20 bits of
            // CPUID.01H:EAX:
            //
            //   bits 19:16: Extended Model ID
            //   bits 13:12: Processor Type
            //   bits 11:8:  Family ID
            //   bits 7:4:   Model ID
            //   bits 3:0:   Stepping ID
            //
            // The low nibble of byte 3 is the platform component of FMSPC.
            // The remaining FMSPC bytes should be preserved for exact
            // collateral identity, but are not needed to compute
            // DisplayFamily_DisplayModel.
            let eax20 = ((fmspc.0[1] as u32) << 12)
                | ((fmspc.0[2] as u32) << 4)
                | ((fmspc.0[3] as u32) >> 4);

            let stepping_id = (eax20 & 0x0f) as u8;
            let model_id = ((eax20 >> 4) & 0x0f) as u8;
            let family_id = ((eax20 >> 8) & 0x0f) as u8;
            let processor_type = ((eax20 >> 12) & 0x03) as u8;
            let extended_model_id = ((eax20 >> 16) & 0x0f) as u8;

            // All current SGX PCS FMSPCs are Intel family 06h. Figure out how
            // to handle this if they every release something with a different
            // family id.
            if family_id != 0x06 {
                panic!("handle this");
            }

            let display_family = family_id;
            let display_model = (extended_model_id << 4) | model_id;

            Self {
                fmspc,
                cpuid_01_eax_low20: eax20,
                family_id,
                model_id,
                stepping_id,
                extended_model_id,
                processor_type,
                display_family,
                display_model,
            }
        }
    }

    impl fmt::Display for FmspcCpuid {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Self {
                fmspc,
                cpuid_01_eax_low20,
                family_id,
                model_id,
                stepping_id,
                extended_model_id,
                processor_type,
                display_family,
                display_model,
            } = self;
            let intel_family_name = self.intel_family_name();
            write!(
                f,
                "{display_family:02X}_{display_model:02X}H \
                 CPUID.01H:EAX[19:0]=0x{cpuid_01_eax_low20:05X} \
                 F/M/S/eM/P={family_id:X}/{model_id:X}/{stepping_id:X}/{extended_model_id:X}/{processor_type:X} \
                 FMSPC={fmspc} {intel_family_name:<48}"
            )
        }
    }

    impl FmspcCpuid {
        fn intel_family_name(&self) -> String {
            let stepping_id = self.stepping_id;

            // Names from:
            // - arch/x86/include/asm/intel-family.h in Linux
            // - [Intel Platform Security Guidance - Affected Processors](https://www.intel.com/content/www/us/en/developer/topic-technology/software-security-guidance/processors-affected-consolidated-product-cpu-model.html).
            //   (in .csv format: <https://github.com/intel/Intel-affected-processor-list/blob/main/Intel_affected_processor_list.csv>)
            match (self.display_family, self.display_model) {
                (0x06, 0x6A) =>
                    format!("INTEL_ICELAKE_X (Sunny Cove, s:{stepping_id:X})"),
                (0x06, 0x6C) =>
                    format!("INTEL_ICELAKE_D (Sunny Cove, s:{stepping_id:X})"),
                (0x06, 0x7A) => {
                    format!(
                        "INTEL_ATOM_GOLDMONT_PLUS (Gemini Lake, s:{stepping_id:X})"
                    )
                }
                (0x06, 0x7E) =>
                    format!("INTEL_ICELAKE_L (Sunny Cove, s:{stepping_id:X})"),
                (0x06, 0x8E) => match stepping_id {
                    0x0A => "INTEL_KABYLAKE_L / COFFEELAKE_L (Sky Lake, s:A)"
                        .to_owned(),
                    0x0B | 0x0C => {
                        format!(
                            "INTEL_KABYLAKE_L / WHISKEYLAKE_L (Sky Lake, s:{stepping_id:X})"
                        )
                    }
                    _ => format!(
                        "INTEL_KABYLAKE_L (Sky Lake, s:{stepping_id:X})"
                    ),
                },
                (0x06, 0x8F) => {
                    format!(
                        "INTEL_SAPPHIRERAPIDS_X (Golden Cove, s:{stepping_id:X})"
                    )
                }
                (0x06, 0x9E) => match stepping_id {
                    0x0A..=0x0D => {
                        format!(
                            "INTEL_KABYLAKE / COFFEELAKE (Sky Lake, s:{stepping_id:X})"
                        )
                    }
                    _ =>
                        format!("INTEL_KABYLAKE (Sky Lake, s:{stepping_id:X})"),
                },
                (0x06, 0xA5) =>
                    format!("INTEL_COMETLAKE (Sky Lake, s:{stepping_id:X})"),
                (0x06, 0xA7) => format!(
                    "INTEL_ROCKETLAKE (Cypress Cove, s:{stepping_id:X})"
                ),
                (0x06, 0xAD) =>
                    format!("INTEL_GRANITERAPIDS_X (s:{stepping_id:X})"),
                (0x06, 0xAE) =>
                    format!("INTEL_GRANITERAPIDS_D (s:{stepping_id:X})"),
                (0x06, 0xAF) => {
                    format!(
                        "INTEL_ATOM_CRESTMONT_X (Sierra Forest, s:{stepping_id:X})"
                    )
                }
                (0x06, 0xCF) =>
                    format!("INTEL_EMERALDRAPIDS_X (s:{stepping_id:X})"),
                _ => format!("unknown Intel family (s:{stepping_id:X})"),
            }
        }

        fn is_older_than_icelake(&self) -> bool {
            self.display_family == 0x06
                && matches!(self.display_model, 0x7A | 0x8E | 0x9E | 0xA5)
        }
    }
}
