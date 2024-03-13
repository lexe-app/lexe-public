//! Manage self-signed x509 certificate containing enclave remote attestation
//! endorsements.

use std::{borrow::Cow, fmt};

use anyhow::Context;
use rcgen::RcgenError;
use yasna::models::ObjectIdentifier;

use crate::{ed25519, hex, rng::Crng, tls};

/// An x509 certificate containing remote attestation endorsements.
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

// -- impl AttestationCert -- //

impl AttestationCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe remote attestation cert";

    /// Sample a fresh cert keypair, gather remote attestation evidence, and
    /// embed these in an ephemeral TLS cert which has the remote attestation
    /// evidence embedded, and which is bound to the given DNS name.
    pub fn generate(
        rng: &mut impl Crng,
        dns_name: String,
    ) -> anyhow::Result<Self> {
        // Generate a fresh key pair, which we'll use for the attestation cert.
        let key_pair = ed25519::KeyPair::from_rng(rng);

        // Get our enclave measurement and cert pk quoted by the enclave
        // platform. This process binds the cert pk to the quote evidence. When
        // a client verifies the Quote, they can also trust that the cert was
        // generated on a valid, genuine enclave. Once this trust is settled,
        // they can safely provision secrets onto the enclave via the newly
        // established secure TLS channel.
        //
        // Get the quote as an x509 cert extension that we'll embed in our
        // self-signed provisioning cert.
        let attestation_ext =
            super::quote::quote_enclave(rng, key_pair.public_key())
                .context("Failed to quote enclave")?;
        let cert_ext = attestation_ext.to_cert_extension();

        let now = time::OffsetDateTime::now_utc();
        let not_before = now - time::Duration::HOUR;
        let not_after = now + time::Duration::HOUR;
        let subject_alt_names = vec![rcgen::SanType::DnsName(dns_name)];

        let cert = tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            subject_alt_names,
            &key_pair,
            |params: &mut rcgen::CertificateParams| {
                params.custom_extensions = vec![cert_ext];
            },
        );

        Ok(Self(cert))
    }

    /// DER-encode and self-sign the attestation cert.
    pub fn serialize_der_self_signed(
        &self,
    ) -> Result<rustls::Certificate, RcgenError> {
        self.0.serialize_der().map(rustls::Certificate)
    }

    /// DER-encode the attestation cert's private key.
    pub fn serialize_key_der(&self) -> rustls::PrivateKey {
        rustls::PrivateKey(self.0.serialize_private_key_der())
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
    use crate::rng::WeakRng;

    #[test]
    fn test_keypair_pk_len() {
        let key_pair = rcgen::KeyPair::generate(&rcgen::PKCS_ED25519).unwrap();
        let pk_raw = key_pair.public_key_raw();
        // sanity check ed25519 pk length is what we expect
        assert_eq!(pk_raw.len(), 32);
    }

    #[test]
    fn test_gen_cert() {
        let mut rng = WeakRng::from_u64(20240217);
        let dns_name = "hello.world".to_owned();
        let cert = AttestationCert::generate(&mut rng, dns_name).unwrap();
        let _cert_bytes = cert.serialize_der_self_signed().unwrap();
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
}
