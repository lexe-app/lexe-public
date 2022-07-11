//! Utilities for working w/ ed25519 keys (used to sign x509 certs for now).

use std::fmt;

use asn1_rs::{oid, Oid};
use rcgen::RcgenError;
use ring::signature::KeyPair as _;
use thiserror::Error;
use x509_parser::x509::SubjectPublicKeyInfo;

use crate::hex;

/// The standard PKCS OID for Ed25519
#[rustfmt::skip]
pub const PKCD_OID: Oid<'static> = oid!(1.3.101.112);

#[derive(Debug, Error)]
pub enum Error {
    #[error("ed25519 public key must be exactly 32 bytes")]
    InvalidPubkeyLength,

    #[error("the algorithm OID doesn't match the standard ed25519 OID")]
    UnexpectedAlgorithm,
}

/// An ed25519 public key
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        // TODO(phlip9): check for malleability/small-order subgroup
        // https://github.com/aptos-labs/aptos-core/blob/3f437b5597b5d537d03755e599f395a2242f2b91/crates/aptos-crypto/src/ed25519.rs#L358
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 32 {
            return Err(Error::InvalidPubkeyLength);
        }

        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(bytes);
        Ok(Self::new(pubkey))
    }
}

impl TryFrom<&SubjectPublicKeyInfo<'_>> for PublicKey {
    type Error = Error;

    fn try_from(spki: &SubjectPublicKeyInfo<'_>) -> Result<Self, Self::Error> {
        let alg = &spki.algorithm;
        if !(alg.oid() == &PKCD_OID) {
            return Err(Error::UnexpectedAlgorithm);
        }

        Self::try_from(spki.subject_public_key.as_ref())
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.as_bytes()))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ed25519::PublicKey")
            .field(&hex::display(self.as_bytes()))
            .finish()
    }
}

// TODO(phlip9): patch ring/rcgen so `Ed25519KeyPair` derives `Default` so we
// can wrap it in `Secret<..>`

pub fn from_seed(seed: &[u8; 32]) -> rcgen::KeyPair {
    let key_pair =
        ring::signature::Ed25519KeyPair::from_seed_unchecked(seed.as_slice())
            .expect(
                "This should never fail, as the secret is exactly 32 bytes",
            );
    let key_bytes = seed.as_slice();
    let pubkey_bytes = key_pair.public_key().as_ref();
    let pkcs8_bytes = serialize_pkcs8(key_bytes, pubkey_bytes);

    rcgen::KeyPair::try_from(pkcs8_bytes).expect(
        "Deserializing a freshly serialized ed25519 key pair should never fail",
    )
}

pub fn verify_compatible(
    key_pair: rcgen::KeyPair,
) -> Result<rcgen::KeyPair, RcgenError> {
    if key_pair.is_compatible(&rcgen::PKCS_ED25519) {
        Ok(key_pair)
    } else {
        Err(RcgenError::UnsupportedSignatureAlgorithm)
    }
}

// Note: The `PKCS_TEMPLATE_PREFIX` and `PKCS_TEMPLATE_MIDDLE` are pulled from
// this pkcs8 "template" file in the `ring` repo.
//
// $ hexdump -C ring/src/ec/curve25519/ed25519/ed25519_pkcs8_v2_template.der
// 00000000  30 53 02 01 01 30 05 06  03 2b 65 70 04 22 04 20
// 00000010  a1 23 03 21 00

const PKCS_TEMPLATE_PREFIX: &[u8] = &[
    0x30, 0x53, 0x02, 0x01, 0x01, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70,
    0x04, 0x22, 0x04, 0x20,
];
const PKCS_TEMPLATE_MIDDLE: &[u8] = &[0xa1, 0x23, 0x03, 0x21, 0x00];
const PKCS_TEMPLATE_KEY_IDX: usize = 16;

/// Formats a key pair as `prefix || key || middle || pubkey`, where `prefix`
/// and `middle` are two pre-computed blobs.
///
/// Note: adapted from `ring`, which doesn't let you serialize as pkcs#8 via
/// any public API...
fn serialize_pkcs8(private_key: &[u8], public_key: &[u8]) -> Vec<u8> {
    let len = PKCS_TEMPLATE_PREFIX.len()
        + private_key.len()
        + PKCS_TEMPLATE_MIDDLE.len()
        + public_key.len();
    let mut out = vec![0u8; len];
    let key_start_idx = PKCS_TEMPLATE_KEY_IDX;

    let prefix = PKCS_TEMPLATE_PREFIX;
    let middle = PKCS_TEMPLATE_MIDDLE;

    let key_end_idx = key_start_idx + private_key.len();
    out[..key_start_idx].copy_from_slice(prefix);
    out[key_start_idx..key_end_idx].copy_from_slice(private_key);
    out[key_end_idx..(key_end_idx + middle.len())].copy_from_slice(middle);
    out[(key_end_idx + middle.len())..].copy_from_slice(public_key);

    out
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;
    use ring::signature::{
        Ed25519KeyPair, EdDSAParameters, VerificationAlgorithm,
    };

    use super::*;

    #[test]
    fn test_serialize_pkcs8() {
        let seed = [0x42; 32];
        let key_pair1 =
            Ed25519KeyPair::from_seed_unchecked(seed.as_slice()).unwrap();
        let pubkey_bytes: &[u8] = key_pair1.public_key().as_ref();

        let key_pair1_bytes =
            serialize_pkcs8(seed.as_slice(), key_pair1.public_key().as_ref());

        let key_pair2 = Ed25519KeyPair::from_pkcs8(&key_pair1_bytes).unwrap();

        let msg: &[u8] = b"hello, world".as_slice();
        let sig = key_pair2.sign(msg);
        let sig_bytes: &[u8] = sig.as_ref();

        EdDSAParameters
            .verify(pubkey_bytes.into(), msg.into(), sig_bytes.into())
            .unwrap();
    }

    #[test]
    fn test_from_seed() {
        proptest!(|(seed in any::<[u8; 32]>())| {
            // should never panic
            let key_pair = crate::ed25519::from_seed(&seed);
            assert!(key_pair.is_compatible(&rcgen::PKCS_ED25519));
        })
    }

    #[test]
    fn test_from_rcgen() {
        proptest!(|(seed in any::<[u8; 32]>())| {
            let key_pair = crate::ed25519::from_seed(&seed);
            let pubkey_bytes = key_pair.public_key_raw();
            let pubkey = PublicKey::try_from(pubkey_bytes).unwrap();
            assert_eq!(pubkey.as_bytes(), pubkey_bytes);
        });
    }
}
