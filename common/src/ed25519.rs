//! Ed25519 key pairs, signatures, and public keys.
//!
//!
//! ## Why not use [`ring`] directly
//!
//! * [`ring`]'s APIs are often too limited or too inconvenient.
//!
//!
//! ## Why ed25519
//!
//! * More compact pubkeys (32 B) and signatures (64 B) than RSA (aside: please
//!   don't use RSA ever).
//!
//! * Faster sign (~3x) + verify (~2x) than ECDSA/secp256k1.
//!
//! * Deterministic signatures. No key leakage on accidental secret nonce leak
//!   or reuse.
//!
//! * Better side-channel attack resistance.
//!
//!
//! ## Why not ed25519
//!
//! * Deterministic signatures more vulnerable to fault injection attacks.
//!
//!   We could consider using hedged signatures `Sign(random-nonce || message)`.
//!   Fortunately hedged signatures aren't fundamentally unsafe when nonces are
//!   reused.
//!
//! * Small-order torsion subgroup -> signature malleability fun

// TODO(phlip9): patch ring/rcgen so `Ed25519KeyPair`/`rcgen::KeyPair` derive
//               `Zeroize`.

use std::fmt;

use asn1_rs::{oid, Oid};
use rcgen::RcgenError;
use ref_cast::RefCast;
use ring::signature::KeyPair as _;
use thiserror::Error;
use x509_parser::x509::SubjectPublicKeyInfo;

use crate::hex::{self, FromHex};
use crate::rng::Crng;
use crate::{const_assert_usize_eq, const_ref_cast};

/// The standard PKCS OID for Ed25519
#[rustfmt::skip]
pub const PKCS_OID: Oid<'static> = oid!(1.3.101.112);

pub const SECRET_KEY_LEN: usize = 32;
pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;

/// An ed25519 secret key and public key.
///
/// Applications should always sign with a *key pair* rather than passing in
/// the secret key and public key separately, to avoid attacks like
/// [attacker controlled pubkey signing](https://github.com/MystenLabs/ed25519-unsafe-libs).
pub struct KeyPair {
    /// The ring key pair for actually signing things.
    key_pair: ring::signature::Ed25519KeyPair,
    /// We need to hold on to the seed so we can still serialize the key pair.
    seed: [u8; 32],
}

/// An ed25519 public key.
#[derive(Copy, Clone, PartialEq, Eq, RefCast)]
#[repr(transparent)]
pub struct PublicKey([u8; 32]);

/// An ed25519 signature.
#[derive(Copy, Clone, PartialEq, Eq, RefCast)]
#[repr(transparent)]
pub struct Signature([u8; 64]);

/// A message whose signature we've verified was signed by the given
/// [`ed25519::PublicKey`](PublicKey).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VerifiedMessage<'pk, 'msg> {
    signer: &'pk PublicKey,
    msg: &'msg [u8],
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("ed25519 public key must be exactly 32 bytes")]
    InvalidPkLength,

    #[error("the algorithm OID doesn't match the standard ed25519 OID")]
    UnexpectedAlgorithm,

    #[error("failed deserializing PKCS#8-encoded key pair")]
    DeserializeError,

    #[error("derived public key doesn't match expected public key")]
    PublicKeyMismatch,

    #[error("invalid signature")]
    InvalidSignature,
}

// -- impl KeyPair -- //

impl KeyPair {
    /// Create a new `ed25519::KeyPair` from a random 32-byte seed.
    ///
    /// Use this when deriving a key pair from a KDF like
    /// [`RootSeed`](crate::root_seed::RootSeed).
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let key_pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(
            seed,
        )
        .expect("This should never fail, as the seed is exactly 32 bytes");
        Self {
            seed: *seed,
            key_pair,
        }
    }

    /// Create a new `ed25519::KeyPair` from a random 32-byte seed and the
    /// expected public key. Will return an error if the derived public key
    /// doesn't match.
    pub fn from_seed_and_pubkey(
        seed: &[u8; 32],
        expected_pubkey: &[u8; 32],
    ) -> Result<Self, Error> {
        let key_pair =
            ring::signature::Ed25519KeyPair::from_seed_and_public_key(
                seed.as_slice(),
                expected_pubkey.as_slice(),
            )
            .map_err(|_| Error::PublicKeyMismatch)?;
        Ok(Self {
            seed: *seed,
            key_pair,
        })
    }

    /// Sample a new `ed25519::KeyPair` from a cryptographic RNG.
    ///
    /// Use this when sampling a key pair for the first time or sampling an
    /// ephemeral key pair.
    pub fn from_rng(rng: &mut dyn Crng) -> Self {
        let mut seed = [0u8; 32];
        rng.fill_bytes(seed.as_mut_slice());
        Self::from_seed(&seed)
    }

    /// Convert the current `ed25519::KeyPair` into an [`rcgen::KeyPair`].
    pub fn to_rcgen(&self) -> rcgen::KeyPair {
        let pkcs8_bytes = self.serialize_pkcs8();
        rcgen::KeyPair::try_from(pkcs8_bytes.as_slice()).expect(
            "Deserializing a freshly serialized ed25519 key pair should never fail",
        )
    }

    /// Convert the current `ed25519::KeyPair` into a
    /// [`ring::signature::Ed25519KeyPair`].
    ///
    /// Requires a small intermediate serialization step since [`ring`] key
    /// pairs can't be cloned.
    pub fn to_ring(&self) -> ring::signature::Ed25519KeyPair {
        let pkcs8_bytes = self.serialize_pkcs8();
        ring::signature::Ed25519KeyPair::from_pkcs8(&pkcs8_bytes).unwrap()
    }

    /// Convert the current `ed25519::KeyPair` into a
    /// [`ring::signature::Ed25519KeyPair`] without an intermediate
    /// serialization step.
    pub fn into_ring(self) -> ring::signature::Ed25519KeyPair {
        self.key_pair
    }

    /// Create a new `ed25519::KeyPair` from a short id number.
    ///
    /// NOTE: this should only be used in tests.
    pub fn for_test(id: u64) -> Self {
        const LEN: usize = std::mem::size_of::<u64>();

        let mut seed = [0u8; 32];
        seed[0..LEN].copy_from_slice(id.to_le_bytes().as_slice());
        Self::from_seed(&seed)
    }

    /// Serialize the `ed25519::KeyPair` into a PKCS#8 document.
    pub fn serialize_pkcs8(&self) -> [u8; PKCS_LEN] {
        serialize_pkcs8(&self.seed, self.public_key().as_inner())
    }

    /// Deserialize an `ed25519::KeyPair` from a PKCS#8 document.
    pub fn deserialize_pkcs8(bytes: &[u8]) -> Result<Self, Error> {
        let (seed, expected_pubkey) =
            deserialize_pkcs8(bytes).ok_or(Error::DeserializeError)?;
        Self::from_seed_and_pubkey(seed, expected_pubkey)
    }

    /// The secret key or "seed" that generated this `ed25519::KeyPair`.
    pub fn secret_key(&self) -> &[u8; 32] {
        &self.seed
    }

    /// The [`PublicKey`] for this `KeyPair`.
    pub fn public_key(&self) -> &PublicKey {
        let pubkey_bytes =
            <&[u8; 32]>::try_from(self.key_pair.public_key().as_ref()).unwrap();
        PublicKey::from_ref(pubkey_bytes)
    }

    /// Sign a message with this key pair.
    pub fn sign<'pk, 'msg>(
        &'pk self,
        msg: &'msg [u8],
    ) -> (Signature, VerifiedMessage<'pk, 'msg>) {
        let signer = self.public_key();
        let sig = self.key_pair.sign(msg);
        let sig = Signature::try_from(sig.as_ref()).unwrap();
        (sig, VerifiedMessage { signer, msg })
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ed25519::KeyPair")
            .field("sk", &"..")
            .field("pk", &hex::display(self.public_key().as_slice()))
            .finish()
    }
}

// -- impl PublicKey --- //

impl PublicKey {
    pub const fn new(bytes: [u8; 32]) -> Self {
        // TODO(phlip9): check for malleability/small-order subgroup?
        // https://github.com/aptos-labs/aptos-core/blob/3f437b5597b5d537d03755e599f395a2242f2b91/crates/aptos-crypto/src/ed25519.rs#L358
        Self(bytes)
    }

    pub const fn from_ref(bytes: &[u8; 32]) -> &Self {
        const_ref_cast(bytes)
    }

    /// Verify a message was signed by this public key.
    pub fn verify<'pk, 'msg>(
        &'pk self,
        msg: &'msg [u8],
        sig: &Signature,
    ) -> Result<VerifiedMessage<'pk, 'msg>, Error> {
        self.verify_raw(msg, sig.as_slice())?;
        Ok(VerifiedMessage { signer: self, msg })
    }

    pub fn verify_raw(&self, msg: &[u8], sig: &[u8]) -> Result<(), Error> {
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            self.as_slice(),
        )
        .verify(msg, sig)
        .map_err(|_| Error::InvalidSignature)
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub const fn into_inner(self) -> [u8; 32] {
        self.0
    }

    pub const fn as_inner(&self) -> &[u8; 32] {
        &self.0
    }
}

impl TryFrom<&rcgen::KeyPair> for PublicKey {
    type Error = Error;

    fn try_from(key_pair: &rcgen::KeyPair) -> Result<Self, Self::Error> {
        if !key_pair.is_compatible(&rcgen::PKCS_ED25519) {
            return Err(Error::UnexpectedAlgorithm);
        }

        Self::try_from(key_pair.public_key_raw())
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let pk =
            <[u8; 32]>::try_from(bytes).map_err(|_| Error::InvalidPkLength)?;
        Ok(Self::new(pk))
    }
}

impl TryFrom<&SubjectPublicKeyInfo<'_>> for PublicKey {
    type Error = Error;

    fn try_from(spki: &SubjectPublicKeyInfo<'_>) -> Result<Self, Self::Error> {
        let alg = &spki.algorithm;
        if !(alg.oid() == &PKCS_OID) {
            return Err(Error::UnexpectedAlgorithm);
        }

        Self::try_from(spki.subject_public_key.as_ref())
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8; 32]> for PublicKey {
    fn as_ref(&self) -> &[u8; 32] {
        self.as_inner()
    }
}

impl FromHex for PublicKey {
    fn from_hex(s: &str) -> Result<Self, hex::DecodeError> {
        <[u8; 32]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.as_slice()))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ed25519::PublicKey")
            .field(&hex::display(self.as_slice()))
            .finish()
    }
}

// -- impl Signature -- //

impl Signature {
    pub const fn new(sig: [u8; 64]) -> Self {
        Self(sig)
    }

    pub const fn from_ref(sig: &[u8; 64]) -> &Self {
        const_ref_cast(sig)
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub const fn into_inner(self) -> [u8; 64] {
        self.0
    }

    pub const fn as_inner(&self) -> &[u8; 64] {
        &self.0
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8; 64]> for Signature {
    fn as_ref(&self) -> &[u8; 64] {
        self.as_inner()
    }
}

impl TryFrom<&[u8]> for Signature {
    type Error = Error;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        <[u8; 64]>::try_from(value)
            .map(Signature)
            .map_err(|_| Error::InvalidSignature)
    }
}

impl FromHex for Signature {
    fn from_hex(s: &str) -> Result<Self, hex::DecodeError> {
        <[u8; 64]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.as_slice()))
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ed25519::Signature")
            .field(&hex::display(self.as_slice()))
            .finish()
    }
}

// -- impl VerifiedMessage -- //

impl<'pk, 'msg> VerifiedMessage<'pk, 'msg> {
    pub fn signer(&self) -> &'pk PublicKey {
        self.signer
    }

    pub fn message(&self) -> &'msg [u8] {
        self.msg
    }
}

// -- random utilities -- //

pub fn verify_compatible(
    key_pair: rcgen::KeyPair,
) -> Result<rcgen::KeyPair, RcgenError> {
    if key_pair.is_compatible(&rcgen::PKCS_ED25519) {
        Ok(key_pair)
    } else {
        Err(RcgenError::UnsupportedSignatureAlgorithm)
    }
}

// -- low-level PKCS#8 serialization/deserialization -- //

// Since ed25519 secret keys and public keys are always serialized with the same
// size and the PKCS#8 v2 serialization always has the same "metadata" bytes,
// we can just manually inline the constant "metadata" bytes here.

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

// The length of an ed25519 key pair serialized as PKCS#8 v2 with embedded
// public key.
const PKCS_LEN: usize = PKCS_TEMPLATE_PREFIX.len()
    + SECRET_KEY_LEN
    + PKCS_TEMPLATE_MIDDLE.len()
    + PUBLIC_KEY_LEN;

// Ensure this doesn't accidentally change.
const_assert_usize_eq!(PKCS_LEN, 85);

/// Formats a key pair as `prefix || key || middle || pk`, where `prefix`
/// and `middle` are two pre-computed blobs.
///
/// Note: adapted from `ring`, which doesn't let you serialize as pkcs#8 via
/// any public API...
fn serialize_pkcs8(
    secret_key: &[u8; 32],
    public_key: &[u8; 32],
) -> [u8; PKCS_LEN] {
    let mut out = [0u8; PKCS_LEN];
    let key_start_idx = PKCS_TEMPLATE_KEY_IDX;

    let prefix = PKCS_TEMPLATE_PREFIX;
    let middle = PKCS_TEMPLATE_MIDDLE;

    let key_end_idx = key_start_idx + secret_key.len();
    out[..key_start_idx].copy_from_slice(prefix);
    out[key_start_idx..key_end_idx].copy_from_slice(secret_key);
    out[key_end_idx..(key_end_idx + middle.len())].copy_from_slice(middle);
    out[(key_end_idx + middle.len())..].copy_from_slice(public_key);

    out
}

/// Deserialize the seed and pubkey for a key pair from its PKCS#8-encoded
/// bytes.
fn deserialize_pkcs8(bytes: &[u8]) -> Option<(&[u8; 32], &[u8; 32])> {
    if bytes.len() != PKCS_LEN {
        return None;
    }

    let seed_mid_pubkey = bytes.strip_prefix(PKCS_TEMPLATE_PREFIX)?;
    let (seed, mid_pubkey) = seed_mid_pubkey.split_at(SECRET_KEY_LEN);
    let pubkey = mid_pubkey.strip_prefix(PKCS_TEMPLATE_MIDDLE)?;

    let seed = <&[u8; 32]>::try_from(seed).unwrap();
    let pubkey = <&[u8; 32]>::try_from(pubkey).unwrap();

    Some((seed, pubkey))
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;
    use proptest::strategy::Strategy;

    use super::*;

    fn arb_seed() -> impl Strategy<Value = [u8; 32]> {
        any::<[u8; 32]>()
    }

    fn arb_key_pair() -> impl Strategy<Value = KeyPair> {
        arb_seed().prop_map(|seed| KeyPair::from_seed(&seed))
    }

    #[test]
    fn test_serde_pkcs8_roundtrip() {
        proptest!(|(seed in arb_seed())| {
            let key_pair1 = KeyPair::from_seed(&seed);
            let key_pair_bytes = key_pair1.serialize_pkcs8();
            let key_pair2 = KeyPair::deserialize_pkcs8(key_pair_bytes.as_slice()).unwrap();

            assert_eq!(key_pair1.secret_key(), key_pair2.secret_key());
            assert_eq!(key_pair1.public_key(), key_pair2.public_key());
        });
    }

    #[test]
    fn test_deserialize_pkcs8_different_lengths() {
        for size in 0..=256 {
            let bytes = vec![0x42_u8; size];
            let _ = deserialize_pkcs8(&bytes);
        }
    }

    #[test]
    fn test_to_rcgen() {
        proptest!(|(key_pair in arb_key_pair())| {
            let rcgen_key_pair = key_pair.to_rcgen();
            assert_eq!(rcgen_key_pair.public_key_raw(), key_pair.public_key().as_slice());
        });
    }

    // See: [RFC 8032 (EdDSA) > Test Vectors](https://www.rfc-editor.org/rfc/rfc8032.html#page-25)
    #[test]
    fn test_ed25519_test_vector() {
        let sk: [u8; 32] = hex::decode_const(
            b"c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        );
        let pk: [u8; 32] = hex::decode_const(
            b"fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        );
        let msg: [u8; 2] = hex::decode_const(b"af82");
        let sig: [u8; 64] = hex::decode_const(b"6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a");

        let key_pair = KeyPair::from_seed(&sk);
        assert_eq!(key_pair.public_key().as_inner(), &pk);

        let (sig2, verified_msg) = key_pair.sign(&msg);
        assert_eq!(&sig, sig2.as_inner());

        let verified_msg2 = key_pair.public_key().verify(&msg, &sig2).unwrap();
        assert_eq!(verified_msg, verified_msg2);
    }

    #[test]
    fn test_sign_verify() {
        proptest!(|(key_pair in arb_key_pair(), msg in any::<Vec<u8>>())| {
            let pubkey = key_pair.public_key();

            let (sig, verified_msg) = key_pair.sign(&msg);
            assert_eq!(verified_msg.signer(), pubkey);

            let verified_msg2 = pubkey.verify(&msg, &sig).unwrap();
            assert_eq!(verified_msg, verified_msg2);
        });
    }
}
