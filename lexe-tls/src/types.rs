//! TLS newtypes.

use std::path::Path;

use anyhow::{Context, ensure};
use base64::Engine as _;
#[cfg(any(test, feature = "test-utils"))]
use common::test_utils::arbitrary;
use common::{ed25519, serde_helpers::hexstr_or_bytes};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rustls::pki_types::{
    CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer,
    pem::{self, PemObject},
};
use serde::{Deserialize, Serialize};

/// Convenience struct to pass around a DER-encoded cert with its private key
/// and the primary DNS name it was bound to.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct DnsCertWithKey {
    pub cert: CertWithKey,

    /// The _primary_ DNS name used by a service. Clients should use this when
    /// making TLS/HTTPS requests.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub dns: String,
}

/// A DER-encoded cert and its private key. Potentially includes signing
/// intermediate or CA certs in its `cert_chain_der`.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct CertWithKey {
    /// The end-entity cert.
    pub cert_der: LxCertificateDer,
    /// The rest of the cert chain, if any.
    ///
    /// # Root Lexe CA key rotation
    ///
    /// 99% of the time, "Lexe CA" TLS does not require an intermediate cert
    /// chain.
    ///
    /// Only if this corresponds to an end-entity cert used to authenticate
    /// ourselves for "Lexe CA" TLS, and we're in the midst of a Root Lexe CA
    /// key rotation, is an intermediate cert chain required, in which case it
    /// should contain the cert of the NEW Lexe CA (signed by the old Lexe CA).
    ///
    /// See the docs on `LexeRootCaCert` for more info.
    pub cert_chain_der: Vec<LxCertificateDer>,
    /// The private key for the end-entity cert.
    pub key_der: LxPrivatePkcs8KeyDer,
}

/// A [`CertificateDer`] which can be serialized and deserialized.
/// Can be constructed from arbitrary bytes; does not enforce any invariants.
// This Arbitrary impl is only used for serde tests and generates invalid certs.
// Feel free to update the impl if needed.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct LxCertificateDer(#[serde(with = "hexstr_or_bytes")] pub Vec<u8>);

/// A [`PrivatePkcs8KeyDer`] which can be serialized and deserialized.
/// Can be constructed from arbitrary bytes; does not enforce any invariants.
#[derive(Clone, Serialize, Deserialize)]
// This Arbitrary impl is only used for serde tests and generates invalid keys.
// Feel free to update the impl if needed.
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct LxPrivatePkcs8KeyDer(#[serde(with = "hexstr_or_bytes")] pub Vec<u8>);

/// Simple newtype for a [`rcgen::KeyPair`] whose signature algorithm has been
/// verified to be [`ed25519`] (its OID matches the standard [`ed25519`] OID).
/// Its primary purpose is to prevent unnecessary error handling.
pub struct EdRcgenKeypair(rcgen::KeyPair);

// --- impl CertWithKey --- //

impl CertWithKey {
    /// Converts self into the parameters required by [`rustls::ConfigBuilder`].
    pub fn into_chain_and_key(
        mut self,
    ) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        // NOTE: The end-entity cert needs to go *first* in this Vec, followed
        // by intermediate certs (if any).
        self.cert_chain_der.insert(0, self.cert_der);
        let rustls_cert_chain = self
            .cert_chain_der
            .into_iter()
            .map(CertificateDer::from)
            .collect();
        (rustls_cert_chain, self.key_der.into())
    }

    /// Constructs a new [`CertWithKey`] from a cert chain and private key.
    ///
    /// `cert_chain_der` must contain at least one cert, and the first cert in
    /// `cert_chain_der` must be the end-entity cert.
    pub fn from_chain_and_key(
        mut cert_chain_der: Vec<LxCertificateDer>,
        key_der: LxPrivatePkcs8KeyDer,
    ) -> anyhow::Result<Self> {
        let cert_der = cert_chain_der
            .drain(..1)
            .next()
            .context("Cert chain must contain at the end-entity cert")?;
        Ok(Self {
            cert_der,
            cert_chain_der,
            key_der,
        })
    }

    /// Parses a PEM-encoded cert chain and private key.
    pub fn from_pem_slices(
        cert_chain_pem: &[u8],
        key_pem: &[u8],
    ) -> anyhow::Result<Self> {
        let cert_chain_der = LxCertificateDer::pem_slice_iter(cert_chain_pem)
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse PEM cert chain")?;
        let key_der = LxPrivatePkcs8KeyDer::from_pem_slice(key_pem)
            .context("Failed to parse PEM private key")?;
        Self::from_chain_and_key(cert_chain_der, key_der)
    }

    /// Reads and parses a PEM-encoded cert chain and private key from files.
    pub fn from_pem_files(
        cert_chain_path: &Path,
        key_path: &Path,
    ) -> anyhow::Result<Self> {
        let cert_chain_der_iter =
            LxCertificateDer::pem_file_iter(cert_chain_path)
                .with_context(|| cert_chain_path.display().to_string())
                .context("Failed to read PEM cert chain file")?;
        let cert_chain_der = cert_chain_der_iter
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| cert_chain_path.display().to_string())
            .context("Failed to parse PEM cert chain")?;
        let key_der = LxPrivatePkcs8KeyDer::from_pem_file(key_path)
            .with_context(|| key_path.display().to_string())
            .context("Failed to read and parse PEM private key")?;
        Self::from_chain_and_key(cert_chain_der, key_der)
    }
}

// --- impl LxCertificateDer --- //

impl LxCertificateDer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn serialize_pem(&self) -> String {
        let padding = true;
        let b64_len = base64::encoded_len(self.0.len(), padding).unwrap();
        let mut pem = String::with_capacity(b64_len + 56);

        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        base64::engine::general_purpose::STANDARD
            .encode_string(&self.0, &mut pem);
        pem.push_str("\n-----END CERTIFICATE-----\n");
        pem
    }
}

// We can parse these out of `-----BEGIN CERTIFICATE-----` PEM sections.
impl PemObject for LxCertificateDer {
    fn from_pem(kind: pem::SectionKind, der: Vec<u8>) -> Option<Self> {
        match kind {
            pem::SectionKind::Certificate => Some(Self(der)),
            _ => None,
        }
    }
}

/// We intentionally avoid the reverse impls because they require re-allocation.
impl From<LxCertificateDer> for CertificateDer<'static> {
    fn from(lx_cert: LxCertificateDer) -> Self {
        Self::from(lx_cert.0)
    }
}
impl<'der> From<&'der LxCertificateDer> for CertificateDer<'der> {
    fn from(lx_cert: &'der LxCertificateDer) -> Self {
        Self::from(lx_cert.as_slice())
    }
}

// --- impl LxPrivatePkcs8KeyDer --- //

impl LxPrivatePkcs8KeyDer {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

// We can parse these out of `-----BEGIN PRIVATE KEY-----` PEM sections. To keep
// things simple, we'll only support PKCS#8 format and not SEC1 EC format or RSA
// format.
impl PemObject for LxPrivatePkcs8KeyDer {
    fn from_pem(kind: pem::SectionKind, der: Vec<u8>) -> Option<Self> {
        match kind {
            pem::SectionKind::PrivateKey => Some(Self(der)),
            _ => None,
        }
    }
}

/// We intentionally avoid the reverse impls because they require re-allocation.
impl From<LxPrivatePkcs8KeyDer> for PrivateKeyDer<'static> {
    fn from(lx_key: LxPrivatePkcs8KeyDer) -> Self {
        Self::from(PrivatePkcs8KeyDer::from(lx_key.0))
    }
}
impl<'der> From<&'der LxPrivatePkcs8KeyDer> for PrivateKeyDer<'der> {
    fn from(lx_key: &'der LxPrivatePkcs8KeyDer) -> Self {
        Self::from(PrivatePkcs8KeyDer::from(lx_key.as_bytes()))
    }
}

/// We intentionally avoid the reverse impls because they require re-allocation.
impl From<LxPrivatePkcs8KeyDer> for PrivatePkcs8KeyDer<'static> {
    fn from(lx_key: LxPrivatePkcs8KeyDer) -> Self {
        Self::from(lx_key.0)
    }
}
impl<'der> From<&'der LxPrivatePkcs8KeyDer> for PrivatePkcs8KeyDer<'der> {
    fn from(lx_key: &'der LxPrivatePkcs8KeyDer) -> Self {
        Self::from(lx_key.as_bytes())
    }
}

// --- impl Ed25519KeyPair --- //

impl EdRcgenKeypair {
    /// Equivalent to [`ed25519::KeyPair::to_rcgen`] or using the [`From`] impl.
    pub fn from_ed25519(key_pair: &ed25519::KeyPair) -> Self {
        Self(key_pair.to_rcgen())
    }

    /// Errors if the [`rcgen::KeyPair`] doesn't match the standard ed25519 OID.
    /// Equivalent to using the [`TryFrom`] impl.
    pub fn try_from_rcgen(key_pair: rcgen::KeyPair) -> anyhow::Result<Self> {
        ensure!(
            *key_pair.algorithm() == rcgen::PKCS_ED25519,
            "rcgen::KeyPair doesn't match ed25519 OID",
        );

        Ok(Self(key_pair))
    }

    pub fn as_inner(&self) -> &rcgen::KeyPair {
        &self.0
    }

    pub fn into_inner(self) -> rcgen::KeyPair {
        self.0
    }
}

impl From<ed25519::KeyPair> for EdRcgenKeypair {
    fn from(key_pair: ed25519::KeyPair) -> Self {
        Self::from_ed25519(&key_pair)
    }
}
impl From<&ed25519::KeyPair> for EdRcgenKeypair {
    fn from(key_pair: &ed25519::KeyPair) -> Self {
        Self::from_ed25519(key_pair)
    }
}

impl TryFrom<rcgen::KeyPair> for EdRcgenKeypair {
    type Error = anyhow::Error;
    fn try_from(key_pair: rcgen::KeyPair) -> Result<Self, Self::Error> {
        Self::try_from_rcgen(key_pair)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_cert_with_key_pem() {
        let cert_chain_pem = r#"
-----BEGIN CERTIFICATE-----
MIID0zCCA1mgAwIBAgISLDggldDv8zKRlvUy0KsseoehMAoGCCqGSM49BAMDMFcx
CzAJBgNVBAYTAlVTMSAwHgYDVQQKExcoU1RBR0lORykgTGV0J3MgRW5jcnlwdDEm
MCQGA1UEAxMdKFNUQUdJTkcpIFB1enpsaW5nIFBhcnNuaXAgRTcwHhcNMjUwOTIy
MTgwNjMyWhcNMjUxMjIxMTgwNjMxWjAnMSUwIwYDVQQDExxmb290ZXN0MS51c3dl
c3QuZGV2LmxleGUuYXBwMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEzjUM/iZ8
gUrBqIJ1cagIWNZf9/tswhm0qQJxKaBoECcBXuxC2ogRhaVWvqzDyN46P+f8tgU5
SNTWvGj/j6seLqOCAjMwggIvMA4GA1UdDwEB/wQEAwIHgDAdBgNVHSUEFjAUBggr
BgEFBQcDAQYIKwYBBQUHAwIwDAYDVR0TAQH/BAIwADAdBgNVHQ4EFgQUWke2DiQT
2Tlk9IcLmNb+qdb3+8AwHwYDVR0jBBgwFoAUpA+UC0RjapmpoNmMZkOxT9ywLEYw
NgYIKwYBBQUHAQEEKjAoMCYGCCsGAQUFBzAChhpodHRwOi8vc3RnLWU3LmkubGVu
Y3Iub3JnLzAnBgNVHREEIDAeghxmb290ZXN0MS51c3dlc3QuZGV2LmxleGUuYXBw
MBMGA1UdIAQMMAowCAYGZ4EMAQIBMDEGA1UdHwQqMCgwJqAkoCKGIGh0dHA6Ly9z
dGctZTcuYy5sZW5jci5vcmcvMzQuY3JsMIIBBQYKKwYBBAHWeQIEAgSB9gSB8wDx
AHYAFuhpwdGV6tfD+Jca4/B2AfeM4badMahSGLaDfzGoFQgAAAGZctCvWwAABAMA
RzBFAiB3YrBYgytvBm4/SRvGLVLbiaptRpNpbBj1sSbjrAPPWwIhANsDr9JeMevw
/FlQ1axMhomZwOY2zd7gNU9G01neUmDxAHcACJgkSwLHn2trJ8xOlTah7UA2VCGa
x4rBeJVynD5OjIcAAAGZctCvOgAABAMASDBGAiEAw1LXYlkFYQ80155/Gaiy8ejZ
qqT/ssKpc9zQjrCN8KUCIQCQy4dginzQklJS0/iJbgwbkwYMhKeBd6bwwd8l/snH
5jAKBggqhkjOPQQDAwNoADBlAjBfkmLja1E25bbZMoi9Rtk3MFHqv6Xlpeeztuk7
qUm1QRHHLwH8NyyjQmRPyV3jHHoCMQCXpbYJG2joeAcP/V2mwYmnaI2kS6EQ5GgM
y5qpma4yhjmJnvcWda1jRDsgAiAJXm0=
-----END CERTIFICATE-----

-----BEGIN CERTIFICATE-----
MIIEmzCCAoOgAwIBAgIQR1zhS092VJ8XK2pNm/t1gDANBgkqhkiG9w0BAQsFADBm
MQswCQYDVQQGEwJVUzEzMDEGA1UEChMqKFNUQUdJTkcpIEludGVybmV0IFNlY3Vy
aXR5IFJlc2VhcmNoIEdyb3VwMSIwIAYDVQQDExkoU1RBR0lORykgUHJldGVuZCBQ
ZWFyIFgxMB4XDTI0MDMxMzAwMDAwMFoXDTI3MDMxMjIzNTk1OVowVzELMAkGA1UE
BhMCVVMxIDAeBgNVBAoTFyhTVEFHSU5HKSBMZXQncyBFbmNyeXB0MSYwJAYDVQQD
Ex0oU1RBR0lORykgUHV6emxpbmcgUGFyc25pcCBFNzB2MBAGByqGSM49AgEGBSuB
BAAiA2IABHu5ddBGjP6Ky/vtPVXikXyYxd8+ua+vISFBc3hJ1Iz/zme8T3C7BQsc
U3WslRgVeI6c2CpEn2pB+5xb2PRVY8u8RoyrtKV7Q0gcUbQ5bMHYJc1Zubn4tcWt
+tAkzx5JoqOCAQAwgf0wDgYDVR0PAQH/BAQDAgGGMB0GA1UdJQQWMBQGCCsGAQUF
BwMCBggrBgEFBQcDATASBgNVHRMBAf8ECDAGAQH/AgEAMB0GA1UdDgQWBBSkD5QL
RGNqmamg2YxmQ7FP3LAsRjAfBgNVHSMEGDAWgBS182Xy/rAKkh/7PH3zRKCsYyXD
FDA2BggrBgEFBQcBAQQqMCgwJgYIKwYBBQUHMAKGGmh0dHA6Ly9zdGcteDEuaS5s
ZW5jci5vcmcvMBMGA1UdIAQMMAowCAYGZ4EMAQIBMCsGA1UdHwQkMCIwIKAeoByG
Gmh0dHA6Ly9zdGcteDEuYy5sZW5jci5vcmcvMA0GCSqGSIb3DQEBCwUAA4ICAQBk
ws0hFFdRM6HYbbeSV+sAX3qiH0GSQCAS3le8ZdEDw0vdLQUqNA8dYd4t2P0tjFg5
3ZVr8MFDQvP0zMyTAROT7SB4/8yG9QTWV9uQ4fMjwRh474EWvdXDMPVIw1W9FhiF
NatXQD9o6Dg3Q91puWUxMOwiux+XkpMRHpFQ/6kHC9O4whjYqvZOYZaRwg0aiAg4
SOnorJMeo2215nAsFWidfJF7WzfUQHRWsmSdJumUf6SSYl2hhB11nFTSHQG75uaG
qz27J/XSP+QiF0BBBR5iK7x2W7vOFG1UTFAredh7SAkJehlHfnNcrFLHGPvkRLb1
gW6BZH7tl2DB3auMuP5MdFytEq88HG83eerp4WRBZ8RL+R3nXDo6fCv1SMr6mzA5
lsytrmDDuWSSRsn/rSkx78h+JtDfBrAz+QAVYa7I49nKRLyhc9RjOGTGpZh2LLbi
Q09bTBIgSN14ZiCBbet8vH5c+PeDRZnBbSZrJn9Ju14m43Z1rmOOcd4VoG1wjD2X
Q3DrM3K1TMn6DWq7Ks1sb+XkoMVKqi++M4bip3PUxdNNfj+ekovaaK1JJsjEDgCp
f6V+ThZrDT72tmOYrus7oQXDglKZ2rJWON5LB/kK5Z9Nn7/uMShxuwxp5rwHf5zb
AQlfAKgotEgPQfmzftRHvTab4vAx+D2u8+NHlzitJg==
-----END CERTIFICATE-----
"#;
        // openssl genpkey -algorithm ed25519 -text
        let key_pem1 = r#"
-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINQpSgZB1XJFtNP5XpqrCOhJIKGZspH21Kv+0qQTXVOU
-----END PRIVATE KEY-----
"#;

        // openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -text
        let key_pem2 = r#"
-----BEGIN PRIVATE KEY-----
MIIBeQIBADCCAQMGByqGSM49AgEwgfcCAQEwLAYHKoZIzj0BAQIhAP////8AAAAB
AAAAAAAAAAAAAAAA////////////////MFsEIP////8AAAABAAAAAAAAAAAAAAAA
///////////////8BCBaxjXYqjqT57PrvVV2mIa8ZR0GsMxTsPY7zjw+J9JgSwMV
AMSdNgiG5wSTamZ44ROdJreBn36QBEEEaxfR8uEsQkf4vOblY6RA8ncDfYEt6zOg
9KE5RdiYwpZP40Li/hp/m47n60p8D54WK84zV2sxXs7LtkBoN79R9QIhAP////8A
AAAA//////////+85vqtpxeehPO5ysL8YyVRAgEBBG0wawIBAQQgmC92fEs29Yxt
VV18aYUGrn3dek/cKw8+NZ9wxHuDWBuhRANCAARzPsNENY/yjsHq35Z3YTDQl/Du
RxLC15EHd3tbFiVqsa7+QyLr9l0G/uvXRPSRZ6IQchnqg1QakmLV6OxuTWwg
-----END PRIVATE KEY-----
"#;

        let cert_with_key = CertWithKey::from_pem_slices(
            cert_chain_pem.as_bytes(),
            key_pem1.as_bytes(),
        )
        .unwrap();
        assert_eq!(cert_with_key.cert_chain_der.len(), 1);

        let cert_with_key = CertWithKey::from_pem_slices(
            cert_chain_pem.as_bytes(),
            key_pem2.as_bytes(),
        )
        .unwrap();
        assert_eq!(cert_with_key.cert_chain_der.len(), 1);
    }
}
