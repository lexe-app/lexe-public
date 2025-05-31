//! TLS newtypes.
//!
//! # Avoiding redundant allocations
//!
//! The DER-encoded data in these types generally flows through the
//! following transformations.
//!
//! Step 1: Into [`Vec<u8>`]
//! - [`rcgen::Certificate::serialize_der`]
//! - [`rcgen::Certificate::serialize_der_with_signer`]
//! - [`rcgen::Certificate::serialize_private_key_der`]
//! - Passed in via args, env, or read from a file
//!
//! Step 2: Into [`LxCertificateDer`] and [`LxPrivatePkcs8KeyDer`]
//! - Wrap [`Vec<u8>`] with [`LxCertificateDer`] or [`LxPrivatePkcs8KeyDer`]
//!
//! Step 3: Into [`CertificateDer<'_>`] and [`PrivateKeyDer<'_>`]
//! - [`CertWithKey::into_chain_and_key`]
//! - `impl From<LxCertificateDer> for CertificateDer<'static>`
//! - `impl From<LxPrivatePkcs8KeyDer> for PrivateKeyDer<'static>`
//! - `impl<'der> From<&'der LxCertificateDer> for CertificateDer<'der>`
//! - `impl<'der> From<&'der LxPrivatePkcs8KeyDer> for PrivateKeyDer<'der>`
//!
//! Trying to move backwards at any step generally requires copying and
//! re-allocation, so try not to do that. For example, avoid premature
//! conversions into [`CertificateDer<'_>`] or [`PrivateKeyDer<'_>`].
//!
//! [`CertWithKey::into_chain_and_key`]: crate::types::CertWithKey::into_chain_and_key
//! [`LxCertificateDer`]: crate::types::LxCertificateDer
//! [`LxPrivatePkcs8KeyDer`]: crate::types::LxPrivatePkcs8KeyDer

use anyhow::ensure;
#[cfg(any(test, feature = "test-utils"))]
use common::test_utils::arbitrary;
use common::{ed25519, serde_helpers::hexstr_or_bytes};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};

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

/// A DER-encoded cert with its private key and possibly the issuing CA cert.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Debug, Eq, PartialEq, Arbitrary)
)]
pub struct CertWithKey {
    pub cert_der: LxCertificateDer,
    pub key_der: LxPrivatePkcs8KeyDer,
    /// The DER-encoded cert of the CA that signed this end-entity cert.
    ///
    /// # Root Lexe CA key rotation
    ///
    /// 99% of the time, "Lexe CA" TLS does not require setting this field.
    ///
    /// Only if this [`CertWithKey`] corresponds to an end-entity cert used to
    /// authenticate ourselves for "Lexe CA" TLS, and we're in the midst of a
    /// Root Lexe CA key rotation, is this field required to be [`Some`], in
    /// which case it should contain the cert of the NEW Lexe CA.
    ///
    /// See the docs on `LexeRootCaCert` for more info.
    pub ca_cert_der: Option<LxCertificateDer>,
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
        self,
    ) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        // NOTE: The end-entity cert needs to go *first* in this Vec.
        let mut cert_chain = vec![self.cert_der.into()];
        if let Some(ca_cert_der) = self.ca_cert_der {
            cert_chain.push(ca_cert_der.into());
        }
        (cert_chain, self.key_der.into())
    }
}

// --- impl LxCertificateDer --- //

impl LxCertificateDer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
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
