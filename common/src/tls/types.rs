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
/// Step 2: Into [`LxCertificateDer`] and [`LxPrivatePkcs8KeyDer`]
/// - [`LxCertificateDer::from`] (from [`Vec<u8>`])
/// - [`LxPrivatePkcs8KeyDer::from`] (from [`Vec<u8>`])
///
/// Step 3: Into [`CertificateDer<'_>`] and [`PrivateKeyDer<'_>`]
/// - `impl From<LxCertificateDer> for CertificateDer<'static>`
/// - `impl From<LxPrivatePkcs8KeyDer> for PrivateKeyDer<'static>`
/// - `impl<'der> From<&'der LxCertificateDer> for CertificateDer<'der>`
/// - `impl<'der> From<&'der LxPrivatePkcs8KeyDer> for PrivateKeyDer<'der>`
///
/// Trying to move backwards at any step generally requires copying and
/// re-allocation, so try not to do that. For example, avoid premature
/// conversions into [`CertificateDer<'_>`] or [`PrivateKeyDer<'_>`].

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
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
pub struct LxCertificateDer(#[serde(with = "hexstr_or_bytes")] Vec<u8>);

/// A [`PrivatePkcs8KeyDer`] which can be serialized and deserialized.
/// Can be constructed from arbitrary bytes; does not enforce any invariants.
#[derive(Clone, Serialize, Deserialize)]
// This Arbitrary impl is only used for serde tests and generates invalid keys.
// Feel free to update the impl if needed.
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct LxPrivatePkcs8KeyDer(#[serde(with = "hexstr_or_bytes")] Vec<u8>);

// --- impl LxCertificateDer --- //

impl LxCertificateDer {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for LxCertificateDer {
    fn from(der_bytes: Vec<u8>) -> Self {
        Self(der_bytes)
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

// --- impl LxPrivatePkcs8KeyDer --- //

impl LxPrivatePkcs8KeyDer {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for LxPrivatePkcs8KeyDer {
    fn from(der_bytes: Vec<u8>) -> Self {
        Self(der_bytes)
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
