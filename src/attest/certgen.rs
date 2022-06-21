//! Generate a self-signed x509 certificate containing enclave remote
//! attestation endorsements.

#![allow(dead_code)]

use rcgen::SanType;
use std::borrow::Cow;
use time::OffsetDateTime;
use yasna::models::ObjectIdentifier;

// TODO(phlip9): attestation extension type should be shared w/ client
// verifiers.

/// The x509 cert extension containing all the evidence a client needs to verify
/// an SGX remote attestation.
///
/// ```
/// SgxAttestationExtension ::= SEQUENCE {
///     QUOTE      OCTET STRING
///     QE_REPORT  OCTET STRING
/// }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct SgxAttestationExtension<'a, 'b> {
    pub quote: Cow<'a, [u8]>,
    pub qe_report: Cow<'b, [u8]>,
    //    3. TODO: QE identity json
    //    4. TODO: QE identity sig + cert chain
    //    5. TODO: locally verifiable Report
}

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

pub struct CertificateParams<'a, 'b> {
    subject_alt_names: Vec<SanType>,
    not_before: OffsetDateTime,
    not_after: OffsetDateTime,
    sgx_attestation: SgxAttestationExtension<'a, 'b>,
}

#[cfg(test)]
mod test {
    use super::*;

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
