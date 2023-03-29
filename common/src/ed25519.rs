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

// TODO(phlip9): Submit PR to ring for `Ed25519Ctx` support so we don't have to
//               pre-hash.

use std::fmt;

use asn1_rs::{oid, Oid};
use bytes::{BufMut, Bytes, BytesMut};
use rcgen::RcgenError;
use ref_cast::RefCast;
use ring::signature::KeyPair as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x509_parser::x509::SubjectPublicKeyInfo;

use crate::hex::{self, FromHex};
use crate::rng::Crng;
use crate::{const_assert_usize_eq, const_ref_cast, sha256};

/// The standard PKCS OID for Ed25519
#[rustfmt::skip]
pub const PKCS_OID: Oid<'static> = oid!(1.3.101.112);

pub const SECRET_KEY_LEN: usize = 32;
pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;

/// 96 B. The added overhead for a signed struct, on top of the serialized
/// struct size.
pub const SIGNED_STRUCT_OVERHEAD: usize = PUBLIC_KEY_LEN + SIGNATURE_LEN;

/// An ed25519 secret key and public key.
///
/// Applications should always sign with a *key pair* rather than passing in
/// the secret key and public key separately, to avoid attacks like
/// [attacker controlled pubkey signing](https://github.com/MystenLabs/ed25519-unsafe-libs).
pub struct KeyPair {
    /// The ring key pair for actually signing things.
    key_pair: ring::signature::Ed25519KeyPair,

    /// Unfortunately, [`ring`] doesn't expose the `seed` after construction,
    /// so we need to hold on to the seed if we ever need to serialize the key
    /// pair later.
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

/// `Signed<T>` is a "proof" that the signature `sig` on a [`Signable`] struct
/// `T` was actually signed by `signer`.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub struct Signed<T: Signable> {
    signer: PublicKey,
    sig: Signature,
    inner: T,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("ed25519 public key must be exactly 32 bytes")]
    InvalidPkLength,

    #[error("the algorithm OID doesn't match the standard ed25519 OID")]
    UnexpectedAlgorithm,

    #[error("failed deserializing PKCS#8-encoded key pair")]
    KeyDeserializeError,

    #[error("derived public key doesn't match expected public key")]
    PublicKeyMismatch,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("error serializing inner struct for signing: {0}")]
    BcsSerialize(bcs::Error),

    #[error("error deserializing inner struct to verify: {0}")]
    BcsDeserialize(bcs::Error),

    #[error("signed struct is too short")]
    SignedTooShort,

    #[error("message was signed with a different key pair than expected")]
    UnexpectedSigner,
}

#[derive(Debug, Error)]
#[error("invalid signature")]
pub struct InvalidSignature;

/// `Signable` types are types that can be signed with
/// [`ed25519::KeyPair::sign_struct`](KeyPair::sign_struct).
///
/// `Signable` types must have a _globally_ unique domain separation value to
/// prevent type confusion attacks. This value is effectively prepended to the
/// signature in order to bind that signature to only this particular type.
pub trait Signable {
    /// Implementors will only need to fill in this value. An example is
    /// `"LEXE-REALM::RootSeed"`, used in the
    /// [`RootSeed`](crate::root_seed::RootSeed).
    ///
    /// Any length is fine, as the value is SHA-256 hashed at compile time to
    /// reduce it to a constant size.
    const DOMAIN_SEPARATOR_STR: &'static [u8];

    /// The actual domain separation value prepended to signatures on this type.
    const DOMAIN_SEPARATOR: [u8; 32] =
        sha256::digest_const(Self::DOMAIN_SEPARATOR_STR).into_inner();
}

// Blanket trait impl for &T.
impl<T: Signable> Signable for &T {
    const DOMAIN_SEPARATOR_STR: &'static [u8] = T::DOMAIN_SEPARATOR_STR;
    const DOMAIN_SEPARATOR: [u8; 32] = T::DOMAIN_SEPARATOR;
}

// -- verify_signed_struct -- //

/// Helper fn to pass to [`ed25519::verify_signed_struct`](verify_signed_struct)
/// that accepts any public key, so long as the signature is OK.
pub fn accept_any_signer(_: &PublicKey) -> bool {
    true
}

/// Verify a serialized and signed [`Signable`] struct. Returns the deserialized
/// struct inside a [`Signed`] proof that it was in fact signed by the
/// associated [`ed25519::PublicKey`](PublicKey).
///
/// Signed struct signatures are created using
/// [`ed25519::KeyPair::sign_struct`](KeyPair::sign_struct).
pub fn verify_signed_struct<'msg, T, F>(
    is_expected_signer: F,
    serialized: &'msg [u8],
) -> Result<Signed<T>, Error>
where
    T: Signable + Deserialize<'msg>,
    F: FnOnce(&'msg PublicKey) -> bool,
{
    let (signer, sig, ser_struct) = deserialize_signed_struct(serialized)?;

    // ensure the signer is expected
    if !is_expected_signer(signer) {
        return Err(Error::UnexpectedSigner);
    }

    // verify the signature on this serialized struct. the sig should also
    // commit to the domain separator for this type.
    verify_signed_struct_inner(signer, sig, ser_struct, &T::DOMAIN_SEPARATOR)
        .map_err(|_| Error::InvalidSignature)?;

    // canonically deserialize the struct; assume it's bcs-serialized
    let inner: T =
        bcs::from_bytes(ser_struct).map_err(Error::BcsDeserialize)?;

    // wrap the deserialized struct in a "proof-carrying" type that can only
    // be instantiated by actually verifying the signature.
    Ok(Signed {
        signer: *signer,
        sig: *sig,
        inner,
    })
}

// NOTE: these fns are intentionally written as separate methods w/o
// any generics to reduce binary size.

fn deserialize_signed_struct(
    serialized: &[u8],
) -> Result<(&PublicKey, &Signature, &[u8]), Error> {
    if serialized.len() < SIGNED_STRUCT_OVERHEAD {
        return Err(Error::SignedTooShort);
    }

    // deserialize signer public key
    let (signer, serialized) = serialized.split_array_ref::<PUBLIC_KEY_LEN>();
    let signer = PublicKey::from_ref(signer);

    // deserialize signature
    let (sig, ser_struct) = serialized.split_array_ref::<SIGNATURE_LEN>();
    let sig = Signature::from_ref(sig);

    Ok((signer, sig, ser_struct))
}

fn verify_signed_struct_inner(
    signer: &PublicKey,
    sig: &Signature,
    ser_struct: &[u8],
    domain_separator: &[u8; 32],
) -> Result<(), InvalidSignature> {
    // ring doesn't let you digest multiple values into the inner SHA-512 digest
    // w/o just allocating + copying, so we do a quick pre-hash outside.

    let msg = sha256::digest_many(&[domain_separator.as_slice(), ser_struct]);
    signer.verify_raw(msg.as_slice(), sig)
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
            deserialize_pkcs8(bytes).ok_or(Error::KeyDeserializeError)?;
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

    /// Sign a raw message with this `KeyPair`.
    pub fn sign_raw(&self, msg: &[u8]) -> Signature {
        let sig = self.key_pair.sign(msg);
        let sig = Signature::try_from(sig.as_ref()).unwrap();
        sig
    }

    /// Canonically serialize and then sign a [`Signable`] struct `T` with this
    /// `ed25519::KeyPair`.
    ///
    /// Returns a buffer that contains the signer [`PublicKey`] and generated
    /// [`Signature`] pre-pended in front of the serialized `T`. Also returns a
    /// [`Signed`] "proof" that asserts this `T` was signed by this key pair.
    ///
    /// Values are serialized using [`bcs`], a small binary format intended for
    /// cryptographic canonical serialization.
    ///
    /// You can verify this signed struct using
    /// [`ed25519::verify_signed_struct`](verify_signed_struct).
    pub fn sign_struct<'a, T: Signable + Serialize>(
        &self,
        value: &'a T,
    ) -> Result<(Vec<u8>, Signed<&'a T>), bcs::Error> {
        let signer = self.public_key();

        let struct_ser_len =
            bcs::serialized_size(value)? + SIGNED_STRUCT_OVERHEAD;
        let mut out = Vec::with_capacity(struct_ser_len);

        // out := signer || signature || serialized struct

        out.extend_from_slice(signer.as_slice());
        out.extend_from_slice([0u8; 64].as_slice());
        bcs::serialize_into(&mut out, value)?;

        // sign this serialized struct using a domain separator that is unique
        // for this type.
        let sig = self.sign_struct_inner(
            &out[SIGNED_STRUCT_OVERHEAD..],
            &T::DOMAIN_SEPARATOR,
        );
        out[PUBLIC_KEY_LEN..SIGNED_STRUCT_OVERHEAD]
            .copy_from_slice(sig.as_slice());

        Ok((
            out,
            Signed {
                signer: *signer,
                sig,
                inner: value,
            },
        ))
    }

    // Use an inner function with no generics to avoid extra code
    // monomorphization.
    fn sign_struct_inner(
        &self,
        serialized: &[u8],
        domain_separator: &[u8],
    ) -> Signature {
        let msg = sha256::digest_many(&[domain_separator, serialized]);
        self.sign_raw(msg.as_slice())
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

    /// Verify some raw bytes were signed by this public key.
    pub fn verify_raw(
        &self,
        msg: &[u8],
        sig: &Signature,
    ) -> Result<(), InvalidSignature> {
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ED25519,
            self.as_slice(),
        )
        .verify(msg, sig.as_slice())
        .map_err(|_| InvalidSignature)
    }

    /// Like [`ed25519::verify_signed_struct`](verify_signed_struct) but only
    /// allows signatures produced by this `ed25519::PublicKey`.
    pub fn verify_self_signed_struct<'msg, T: Signable + Deserialize<'msg>>(
        &self,
        serialized: &'msg [u8],
    ) -> Result<Signed<T>, Error> {
        let accept_self_signer = |signer| signer == self;
        verify_signed_struct(accept_self_signer, serialized)
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

// -- impl Signed -- //

impl<T: Signable> Signed<T> {
    pub fn into_parts(self) -> (PublicKey, Signature, T) {
        (self.signer, self.sig, self.inner)
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn signer(&self) -> &PublicKey {
        &self.signer
    }

    pub fn signature(&self) -> &Signature {
        &self.sig
    }

    pub fn as_ref(&self) -> Signed<&T> {
        Signed {
            signer: self.signer,
            sig: self.sig,
            inner: &self.inner,
        }
    }
}

impl<T: Signable + Serialize> Signed<T> {
    pub fn serialize(&self) -> Result<Bytes, bcs::Error> {
        let mut bytes = BytesMut::new();
        self.serialize_inout(&mut bytes)?;
        Ok(bytes.freeze())
    }

    pub fn serialize_inout(
        &self,
        inout: &mut BytesMut,
    ) -> Result<(), bcs::Error> {
        let struct_ser_len =
            bcs::serialized_size(&self.inner)? + SIGNED_STRUCT_OVERHEAD;
        inout.reserve(struct_ser_len);

        // out := signer || signature || serialized struct

        inout.put_slice(self.signer.as_slice());
        inout.put_slice(self.sig.as_slice());
        bcs::serialize_into(&mut inout.writer(), &self.inner)?;

        Ok(())
    }
}

impl<T: Signable + Clone> Signed<&T> {
    pub fn cloned(&self) -> Signed<T> {
        Signed {
            signer: self.signer,
            sig: self.sig,
            inner: self.inner.clone(),
        }
    }
}

impl<T: Signable + Clone> Clone for Signed<T> {
    fn clone(&self) -> Self {
        Self {
            signer: self.signer,
            sig: self.sig,
            inner: self.inner.clone(),
        }
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
    use proptest::strategy::Strategy;
    use proptest::{prop_assume, proptest};
    use proptest_derive::Arbitrary;

    use super::*;

    fn arb_seed() -> impl Strategy<Value = [u8; 32]> {
        any::<[u8; 32]>().no_shrink()
    }

    fn arb_key_pair() -> impl Strategy<Value = KeyPair> {
        arb_seed().prop_map(|seed| KeyPair::from_seed(&seed))
    }

    #[derive(Arbitrary, Deserialize, Serialize)]
    struct SignableBytes(Vec<u8>);

    impl Signable for SignableBytes {
        const DOMAIN_SEPARATOR_STR: &'static [u8] =
            b"LEXE-REALM::SignableBytes";
    }

    impl fmt::Debug for SignableBytes {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("SignableBytes")
                .field(&hex::display(&self.0))
                .finish()
        }
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
        let pubkey = key_pair.public_key();
        assert_eq!(pubkey.as_inner(), &pk);

        let sig2 = key_pair.sign_raw(&msg);
        assert_eq!(&sig, sig2.as_inner());

        pubkey.verify_raw(&msg, &sig2).unwrap();
    }

    // truncating the signed struct should cause the verification to fail.
    #[test]
    fn test_reject_truncated_sig() {
        proptest!(|(key_pair in arb_key_pair(), msg in any::<SignableBytes>())| {
            let pubkey = key_pair.public_key();

            let (sig, signed) = key_pair.sign_struct(&msg).unwrap();
            let sig2 = signed.serialize().unwrap();
            assert_eq!(&sig, &sig2);

            let _ = pubkey
                .verify_self_signed_struct::<SignableBytes>(&sig)
                .unwrap();

            for trunc_len in 0..SIGNED_STRUCT_OVERHEAD {
                pubkey
                    .verify_self_signed_struct::<SignableBytes>(&sig[..trunc_len])
                    .unwrap_err();
                pubkey
                    .verify_self_signed_struct::<SignableBytes>(&sig[(trunc_len+1)..])
                    .unwrap_err();
            }
        });
    }

    // inserting some random bytes into the signed struct should cause the
    // sig verification to fail
    #[test]
    fn test_reject_pad_sig() {
        let cfg = proptest::test_runner::Config::with_cases(50);
        proptest!(cfg, |(
            key_pair in arb_key_pair(),
            msg in any::<SignableBytes>(),
            padding in any::<Vec<u8>>(),
        )| {
            prop_assume!(!padding.is_empty());

            let pubkey = key_pair.public_key();

            let (sig, signed) = key_pair.sign_struct(&msg).unwrap();
            let sig2 = signed.serialize().unwrap();
            assert_eq!(&sig, &sig2);

            let _ = pubkey
                .verify_self_signed_struct::<SignableBytes>(&sig)
                .unwrap();

            let mut sig2: Vec<u8> = Vec::with_capacity(sig.len() + padding.len());

            for idx in 0..=sig.len() {
                let (left, right) = sig.split_at(idx);

                // sig2 := left || padding || right

                sig2.clear();
                sig2.extend_from_slice(left);
                sig2.extend_from_slice(&padding);
                sig2.extend_from_slice(right);

                pubkey
                    .verify_self_signed_struct::<SignableBytes>(&sig2)
                    .unwrap_err();
            }
        });
    }

    // flipping some random bits in the signed struct should cause the
    // verification to fail.
    #[test]
    fn test_reject_modified_sig() {
        let arb_mutation = any::<Vec<u8>>()
            .prop_filter("can't be empty or all zeroes", |m| {
                !m.is_empty() && !m.iter().all(|x| x == &0u8)
            });

        proptest!(|(
            key_pair in arb_key_pair(),
            msg in any::<SignableBytes>(),
            mut_offset in any::<usize>(),
            mut mutation in arb_mutation,
        )| {
            let pubkey = key_pair.public_key();

            let (mut sig, signed) = key_pair.sign_struct(&msg).unwrap();
            let sig2 = signed.serialize().unwrap();
            assert_eq!(&sig, &sig2);

            mutation.truncate(sig.len());
            prop_assume!(!mutation.is_empty() && !mutation.iter().all(|x| x == &0));

            let _ = pubkey
                .verify_self_signed_struct::<SignableBytes>(&sig)
                .unwrap();

            // xor in the mutation bytes to the signature to modify it. any
            // modified bit should cause the verification to fail.
            for (idx_mut, m) in mutation.into_iter().enumerate() {
                let idx_sig = idx_mut.wrapping_add(mut_offset) % sig.len();
                sig[idx_sig] ^= m;
            }

            pubkey.verify_self_signed_struct::<SignableBytes>(&sig).unwrap_err();
        });
    }

    #[test]
    fn test_sign_verify() {
        proptest!(|(key_pair in arb_key_pair(), msg in any::<Vec<u8>>())| {
            let pubkey = key_pair.public_key();

            let sig = key_pair.sign_raw(&msg);
            pubkey.verify_raw(&msg, &sig).unwrap();
        });
    }

    #[test]
    fn test_sign_verify_struct() {
        #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
        struct Foo(u32);

        impl Signable for Foo {
            const DOMAIN_SEPARATOR_STR: &'static [u8] = b"LEXE-REALM::Foo";
        }

        #[derive(Debug, Deserialize, Serialize)]
        struct Bar(u32);

        impl Signable for Bar {
            const DOMAIN_SEPARATOR_STR: &'static [u8] = b"LEXE-REALM::Bar";
        }

        fn arb_foo() -> impl Strategy<Value = Foo> {
            any::<u32>().prop_map(Foo)
        }

        proptest!(|(key_pair in arb_key_pair(), foo in arb_foo())| {
            let signer = key_pair.public_key();
            let (sig, signed) =
                key_pair.sign_struct::<Foo>(&foo).unwrap();
            let sig2 = signed.serialize().unwrap();
            assert_eq!(&sig, &sig2);

            let signed2 =
                signer.verify_self_signed_struct::<Foo>(&sig).unwrap();
            assert_eq!(signed, signed2.as_ref());

            // trying to verify signature as another type with a valid
            // serialization is prevented by domain separation.

            signer.verify_self_signed_struct::<Bar>(&sig).unwrap_err();
        });
    }
}
