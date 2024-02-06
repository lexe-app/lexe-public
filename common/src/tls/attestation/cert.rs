//! Manage self-signed x509 certificate containing enclave remote attestation
//! endorsements.

use std::{borrow::Cow, fmt};

use rcgen::{date_time_ymd, DnType, RcgenError, SanType};
use yasna::models::ObjectIdentifier;

use crate::{constants, ed25519, hex};

/// An x509 certificate containing remote attestation endorsements, usually
/// owned by the lexe node.
pub struct AttestationCert(rcgen::Certificate);

// TODO(phlip9): attestation extension type should be shared w/ client
// verifiers.

/// The x509 cert extension containing all the evidence a client needs to verify
/// an SGX remote attestation.
///
/// ```asn.1
/// SgxAttestationExtension ::= SEQUENCE {
///     QUOTE      OCTET STRING
///     QE_REPORT  OCTET STRING
/// }
/// ```
#[derive(PartialEq, Eq)]
pub struct SgxAttestationExtension<'a, 'b> {
    pub quote: Cow<'a, [u8]>,
    pub qe_report: Cow<'b, [u8]>,
    //    3. TODO: QE identity json
    //    4. TODO: QE identity sig + cert chain
    //    5. TODO: locally verifiable Report
}

// -- impl CertificateParams -- //

impl AttestationCert {
    /// Generate a new attestation certificate using the given `key_pair`, node
    /// `dns_names`, and enclave remote `attestation` evidence (provided as a
    /// custom x509 cert extension).
    pub fn new(
        key_pair: rcgen::KeyPair,
        dns_names: Vec<String>,
        attestation: rcgen::CustomExtension,
    ) -> Result<Self, RcgenError> {
        // TODO(phlip9): don't know how much DN matters...
        let mut name = constants::lexe_distinguished_name_prefix();
        name.push(DnType::CommonName, "node provisioning cert");

        let subject_alt_names = dns_names
            .into_iter()
            .map(SanType::DnsName)
            .collect::<Vec<_>>();

        let mut params = rcgen::CertificateParams::default();

        params.alg = &rcgen::PKCS_ED25519;
        params.key_pair = Some(ed25519::verify_compatible(key_pair)?);
        // no expiration
        // TODO(phlip9): attest certs should be short lived or ephemeral
        params.not_before = date_time_ymd(1975, 1, 1);
        params.not_after = date_time_ymd(4096, 1, 1);
        params.distinguished_name = name;
        params.subject_alt_names = subject_alt_names;
        params.custom_extensions.push(attestation);

        Ok(Self(rcgen::Certificate::from_params(params)?))
    }

    pub fn serialize_der_signed(&self) -> Result<Vec<u8>, RcgenError> {
        self.0.serialize_der()
    }

    pub fn serialize_key_der(&self) -> Vec<u8> {
        self.0.serialize_private_key_der()
    }
}

// -- impl SgxAttestationExtension -- //

impl<'a, 'b> SgxAttestationExtension<'a, 'b> {
    /// This is the Intel SGX OID prefix + 1337.7
    /// gramine uses the same but 1337.6 to embed the quote.
    pub const OID: &'static [u64] = &[1, 2, 840, 113741, 1337, 7];

    /// DER-encoded OID
    pub const OID_DER: &'static [u8] = &[
        0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF8, 0x4D, 0x8A, 0x39, 0x07,
    ];

    pub fn oid_yasna() -> ObjectIdentifier {
        ObjectIdentifier::from_slice(Self::OID)
    }

    #[rustfmt::skip]
    pub const fn oid_asn1_rs() -> asn1_rs::Oid<'static> {
        // TODO(phlip9): won't parse OID_DER...
        asn1_rs::oid!(1.2.840.113741.1337.7)
    }

    /// Clients that don't understand a critical extension will immediately
    /// reject the cert. Unfortunately, setting this to true seems to break
    /// clients...
    pub const fn is_critical() -> bool {
        false
    }

    /// Serialize the attestation to DER.
    pub fn to_der_bytes(&self) -> Vec<u8> {
        yasna::construct_der(|writer| {
            writer.write_sequence(|writer| {
                writer.next().write_bytes(&self.quote);
                writer.next().write_bytes(&self.qe_report);
            })
        })
    }

    pub fn to_cert_extension(&self) -> rcgen::CustomExtension {
        let mut ext = rcgen::CustomExtension::from_oid_content(
            Self::OID,
            self.to_der_bytes(),
        );
        ext.set_criticality(Self::is_critical());
        ext
    }
}

impl SgxAttestationExtension<'static, 'static> {
    /// Build a dummy attestation for testing on non-SGX platforms
    pub const fn dummy() -> Self {
        Self {
            quote: Cow::Borrowed(b"dummy quote"),
            qe_report: Cow::Borrowed(b"dummy qe_report"),
        }
    }

    /// Deserialize the attestation from DER bytes.
    pub fn from_der_bytes(buf: &[u8]) -> yasna::ASN1Result<Self> {
        yasna::parse_der(buf, |reader| {
            reader.read_sequence(|reader| {
                let quote = reader.next().read_bytes()?;
                let qe_report = reader.next().read_bytes()?;
                Ok(Self {
                    quote: Cow::Owned(quote),
                    qe_report: Cow::Owned(qe_report),
                })
            })
        })
    }
}

impl<'a, 'b> fmt::Debug for SgxAttestationExtension<'a, 'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SgxAttestationExtension")
            .field("quote", &hex::display(&self.quote))
            .field("qe_report", &hex::display(&self.qe_report))
            .finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_keypair_pk_len() {
        let key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ED25519).unwrap();
        let pk_raw = key_pair.public_key_raw();
        // sanity check ed25519 pk length is what we expect
        assert_eq!(pk_raw.len(), 32);
    }

    #[test]
    fn test_gen_cert() {
        let key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ED25519).unwrap();
        let dns_names = vec!["hello.world".to_string()];
        let attestation = SgxAttestationExtension {
            quote: b"aaaaa".as_slice().into(),
            qe_report: b"zzzzzz".as_slice().into(),
        }
        .to_cert_extension();
        let cert =
            AttestationCert::new(key_pair, dns_names, attestation).unwrap();
        let _cert_bytes = cert.serialize_der_signed().unwrap();
        // println!("cert:\n{}", pretty_hex(&cert_bytes));

        // example: `openssl -in cert.pem -text
        //
        // Certificate:
        //     Data:
        //         Version: 3 (0x2)
        //         Serial Number: 7046315113334772949 (0x61c98a4339c914d5)
        //     Signature Algorithm: Ed25519
        //         Issuer: C=US, ST=CA, O=lexe-app, CN=lexe-node
        //         Validity
        //             Not Before: May 22 00:00:00 2022 GMT
        //             Not After : May 22 00:00:00 2032 GMT
        //         Subject: C=US, ST=CA, O=lexe-app, CN=lexe-node
        //         Subject Public Key Info:
        //             Public Key Algorithm: Ed25519
        //             Unable to load Public Key
        //         X509v3 extensions:
        //             X509v3 Subject Alternative Name:
        //                 DNS:hello.world
        //             1.2.840.113741.1337.7:
        //                 0...aaaaa..zzzzzz
        //     Signature Algorithm: Ed25519
        //          7c:4d:d3:40:c5:cf:9c:8b:2f:80:66:37:64:19:2c:51:0a:53:
        //          89:b3:cd:1c:85:5f:99:18:b7:3d:68:ad:48:2c:c2:83:02:79:
        //          c2:79:bf:fb:85:76:5d:58:82:59:0f:43:58:4b:db:b3:b4:ba:
        //          0e:62:cb:55:31:17:95:57:71:00
        // -----BEGIN CERTIFICATE-----
        // MIIBdDCCASagAwIBAgIIYcmKQznJFNUwBQYDK2VwMEIxCzAJBgNVBAYMAlVTMQsw
        // CQYDVQQIDAJDQTESMBAGA1UECgwJbGV4ZS10ZWNoMRIwEAYDVQQDDAlsZXhlLW5v
        // ZGUwHhcNMjIwNTIyMDAwMDAwWhcNMzIwNTIyMDAwMDAwWjBCMQswCQYDVQQGDAJV
        // UzELMAkGA1UECAwCQ0ExEjAQBgNVBAoMCWxleGUtdGVjaDESMBAGA1UEAwwJbGV4
        // ZS1ub2RlMCowBQYDK2VwAyEAzDQWHWaB67h4H0Oz32httyHwv0dz2hdkLizhsfg+
        // ncSjOjA4MBYGA1UdEQQPMA2CC2hlbGxvLndvcmxkMB4GCSqGSIb4TYo5BwQRMA8E
        // BWFhYWFhBAZ6enp6enowBQYDK2VwA0EAfE3TQMXPnIsvgGY3ZBksUQpTibPNHIVf
        // mRi3PWitSCzCgwJ5wnm/+4V2XViCWQ9DWEvbs7S6DmLLVTEXlVdxAA==
        // -----END CERTIFICATE-----
    }

    #[test]
    fn test_sgx_attestation_ext_oid_der_bytes() {
        let oid = SgxAttestationExtension::oid_yasna();
        let oid_der = yasna::encode_der(&oid);
        assert_eq!(SgxAttestationExtension::OID_DER, &oid_der);
    }

    #[test]
    fn test_sgx_attestation_ext_serde() {
        let ext = SgxAttestationExtension {
            quote: b"test".as_slice().into(),
            qe_report: b"foo".as_slice().into(),
        };

        let der_bytes = ext.to_der_bytes();
        let ext2 = SgxAttestationExtension::from_der_bytes(&der_bytes).unwrap();
        assert_eq!(ext, ext2);

        let der_bytes2 = ext2.to_der_bytes();
        assert_eq!(der_bytes, der_bytes2);

        assert_eq!(b"test".as_slice(), ext2.quote.as_ref());
        assert_eq!(b"foo".as_slice(), ext2.qe_report.as_ref());
    }

    #[cfg(target_env = "sgx")]
    #[test]
    #[ignore] // << uncomment to dump fresh attestation cert
    fn dump_attest_cert() {
        use crate::{
            ed25519, enclave,
            rng::WeakRng,
            tls::{attestation, attestation::cert::AttestationCert},
        };

        let mut rng = WeakRng::new();
        let cert_key_pair = ed25519::KeyPair::from_seed(&[0x42; 32]);
        let cert_pk = cert_key_pair.public_key();
        let attestation =
            attestation::quote::quote_enclave(&mut rng, cert_pk).unwrap();
        let dns_names = vec!["localhost".to_string()];

        let attest_cert = AttestationCert::new(
            cert_key_pair.to_rcgen(),
            dns_names,
            attestation,
        )
        .unwrap();

        println!("measurement: '{}'", enclave::measurement());
        println!("cert_pk: '{cert_pk}'");

        let cert_der = attest_cert.serialize_der_signed().unwrap();

        println!("attestation certificate:");
        println!("-----BEGIN CERTIFICATE-----");
        println!("{}", base64::encode(cert_der));
        println!("-----END CERTIFICATE-----");
    }
}
