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
//! - `impl From<Vec<u8>> for LxCertificateDer`
//! - `impl From<Vec<u8>> for LxPrivatePkcs8KeyDer`
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
//! [`CertWithKey::into_chain_and_key`]: crate::tls::types::CertWithKey::into_chain_and_key
//! [`LxCertificateDer`]: crate::tls::types::LxCertificateDer
//! [`LxPrivatePkcs8KeyDer`]: crate::tls::types::LxPrivatePkcs8KeyDer

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{ed25519, hexstr_or_bytes};

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
    /// The DER-encoded cert of the Lexe CA that signed this end-entity cert,
    /// which was itself signed by the old Lexe CA during a Lexe CA rotation.
    ///
    /// 99%+ of the time you can just leave this field as [`None`].
    ///
    /// This field is only required to be [`Some`] if:
    /// 1) This [`CertWithKey`] corresponds to an end-entity cert used to
    ///    authenticate ourselves for "Lexe CA" TLS, i.e. the remote verifier
    ///    requires that [`Self::cert_der`] has been signed by the Lexe CA.
    /// 2) Lexe is undergoing a (very rare) root CA key rotation.
    /// 3) We expect to communicate with remote clients/servers that trust the
    ///    old Lexe CA, but have not yet upgraded to trust the new one.
    ///
    /// If all conditions are met, then this field must contain the *new* Lexe
    /// CA cert, which has been signed by the old Lexe CA. [`Self::cert_der`]
    /// must then be an end-entity cert signed by the *new* CA.
    /// [`CertWithKey::into_chain_and_key`] will then include both of these
    /// certs in the cert chain presented to remote verifiers.
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

    /// Whether [`CertWithKey::cert_der`] is bound to the given DNS.
    /// Returns [`false`] if the certificate failed to parse.
    #[must_use]
    pub fn contains_dns(&self, expected_dns: &str) -> bool {
        // Fake keypair which isn't actually used for validation
        let fake_keypair = ed25519::KeyPair::from_seed(&[69; 32]).to_rcgen();

        // This method is ostensibly for CA certs, but doesn't actually check if
        // the cert is a CA cert, so it should be fine to reuse here
        let cert_params = match rcgen::CertificateParams::from_ca_cert_der(
            self.cert_der.as_slice(),
            fake_keypair,
        ) {
            Ok(params) => params,
            Err(_) => return false,
        };

        cert_params.subject_alt_names.iter().any(|san_type| {
            matches!(
                san_type,
                rcgen::SanType::DnsName(bound_dns) if bound_dns == expected_dns
            )
        })
    }
}

// --- impl LxCertificateDer --- //

impl LxCertificateDer {
    pub fn as_slice(&self) -> &[u8] {
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
        Self::from(lx_cert.as_slice())
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
