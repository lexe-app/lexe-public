/// TLS newtypes.
///
/// # Avoiding redundant allocations
///
/// The DER-encoded data in these types generally flows through the
/// following transformations.
///
/// Step 1: Into [`Vec<u8>`]
/// - [`rcgen::Certificate::serialize_der`]
/// - [`rcgen::Certificate::serialize_der_with_signer`]
/// - [`rcgen::Certificate::serialize_private_key_der`]
/// - Passed in via args, env, or read from a file
///
/// Step 2: Into [`LxCertificateDer`] and [`LxPrivateKeyDer`]
/// - [`LxCertificateDer::new`] or via [`From<Vec<u8>>`] impl
/// - [`LxPrivateKeyDer::new`]
///
/// Step 3: Into [`CertificateDer<'_>`] and [`PrivateKeyDer<'_>`]
/// - `impl From<LxCertificateDer> for CertificateDer<'static>`
/// - `impl From<LxPrivateKeyDer> for PrivateKeyDer<'static>`
/// - `impl<'der> From<&'der LxCertificateDer> for CertificateDer<'der>`
/// - `impl<'der> From<&'der LxPrivateKeyDer> for PrivateKeyDer<'der>`
///
/// Trying to move backwards at any step generally requires copying and
/// re-allocation, so try not to do that. For example, avoid premature
/// conversions into [`CertificateDer<'_>`] or [`PrivateKeyDer<'_>`].

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rustls::pki_types::{
    CertificateDer, PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer,
    PrivateSec1KeyDer,
};
use serde::{Deserialize, Serialize};

use crate::hexstr_or_bytes;
#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;

/// Convenience struct to pass around a DER-encoded cert with its private key
/// and the DNS name it was bound to.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct DnsCertWithKey {
    pub cert: CertWithKey,

    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub dns: String,
}

/// A DER-encoded cert with its private key.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct CertWithKey {
    pub cert_der: LxCertificateDer,
    pub key_der: LxPrivateKeyDer,
}

/// A [`CertificateDer`] which can be serialized and deserialized.
/// Can be constructed from arbitrary bytes; does not enforce any invariants.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct LxCertificateDer(Vec<u8>);

/// A [`PrivateKeyDer`] which can be serialized and deserialized.
/// Can be constructed from arbitrary bytes; does not enforce any invariants.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct LxPrivateKeyDer {
    kind: LxPrivateKeyDerKind,
    /// DER-encoded bytes.
    #[serde(with = "hexstr_or_bytes")]
    der_bytes: Vec<u8>,
}

/// Maps to the different [`PrivateKeyDer`] kinds.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxPrivateKeyDerKind {
    Pkcs1,
    Sec1,
    Pkcs8,
}

// --- impl LxCertificateDer --- //

impl LxCertificateDer {
    pub fn new(der_bytes: Vec<u8>) -> Self {
        Self(der_bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for LxCertificateDer {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
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
        Self::from(lx_cert.as_bytes())
    }
}

// --- impl LxPrivateKeyDer --- //

impl LxPrivateKeyDer {
    pub fn new(kind: LxPrivateKeyDerKind, der_bytes: Vec<u8>) -> Self {
        Self { kind, der_bytes }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.der_bytes.as_slice()
    }
}

/// We intentionally avoid the reverse impls because they require re-allocation.
impl From<LxPrivateKeyDer> for PrivateKeyDer<'static> {
    fn from(LxPrivateKeyDer { kind, der_bytes }: LxPrivateKeyDer) -> Self {
        match kind {
            LxPrivateKeyDerKind::Pkcs1 =>
                Self::from(PrivatePkcs1KeyDer::from(der_bytes)),
            LxPrivateKeyDerKind::Sec1 =>
                Self::from(PrivateSec1KeyDer::from(der_bytes)),
            LxPrivateKeyDerKind::Pkcs8 =>
                Self::from(PrivatePkcs8KeyDer::from(der_bytes)),
        }
    }
}
impl<'der> From<&'der LxPrivateKeyDer> for PrivateKeyDer<'der> {
    fn from(lx_key: &'der LxPrivateKeyDer) -> Self {
        match lx_key.kind {
            LxPrivateKeyDerKind::Pkcs1 =>
                Self::from(PrivatePkcs1KeyDer::from(lx_key.as_bytes())),
            LxPrivateKeyDerKind::Sec1 =>
                Self::from(PrivateSec1KeyDer::from(lx_key.as_bytes())),
            LxPrivateKeyDerKind::Pkcs8 =>
                Self::from(PrivatePkcs8KeyDer::from(lx_key.as_bytes())),
        }
    }
}
