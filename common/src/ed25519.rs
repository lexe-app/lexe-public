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

use std::{fmt, str::FromStr};

use asn1_rs::{oid, Oid};
use bytes::{BufMut, Bytes, BytesMut};
use hex::FromHex;
use ref_cast::RefCast;
use ring::signature::KeyPair as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x509_parser::x509::SubjectPublicKeyInfo;
use yasna::{models::ObjectIdentifier, ASN1Error, ASN1ErrorKind};

use crate::{
    ed25519,
    rng::{Crng, RngExt},
};

/// The standard PKCS OID for Ed25519.
/// See "id-Ed25519" in [RFC 8410](https://tools.ietf.org/html/rfc8410).
#[rustfmt::skip]
pub const PKCS_OID: Oid<'static> = oid!(1.3.101.112);
pub const PKCS_OID_SLICE: &[u64] = &[1, 3, 101, 112];

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
#[derive(Copy, Clone, Eq, PartialEq, RefCast)]
#[repr(transparent)]
pub struct PublicKey([u8; 32]);

/// An ed25519 signature.
#[derive(Copy, Clone, Eq, PartialEq, RefCast)]
#[repr(transparent)]
pub struct Signature([u8; 64]);

/// `Signed<T>` is a "proof" that the signature `sig` on a [`Signable`] struct
/// `T` was actually signed by `signer`.
#[derive(Debug, Eq, PartialEq)]
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
    /// `array::pad(*b"LEXE-REALM::RootSeed")`, used in the
    /// [`RootSeed`](crate::root_seed::RootSeed).
    const DOMAIN_SEPARATOR: [u8; 32];
}

// Blanket trait impl for &T.
impl<T: Signable> Signable for &T {
    const DOMAIN_SEPARATOR: [u8; 32] = T::DOMAIN_SEPARATOR;
}

// -- verify_signed_struct -- //

/// Helper fn to pass to [`ed25519::verify_signed_struct`]
/// that accepts any public key, so long as the signature is OK.
pub fn accept_any_signer(_: &PublicKey) -> bool {
    true
}

/// Verify a BCS-serialized and signed [`Signable`] struct.
/// Returns the deserialized struct inside a [`Signed`] proof that it was in
/// fact signed by the associated [`ed25519::PublicKey`].
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
    let (signer, serialized) = serialized
        .split_first_chunk::<PUBLIC_KEY_LEN>()
        .expect("serialized.len() checked above");
    let signer = PublicKey::from_ref(signer);

    // deserialize signature
    let (sig, ser_struct) = serialized
        .split_first_chunk::<SIGNATURE_LEN>()
        .expect("serialized.len() checked above");
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

    pub fn from_seed_owned(seed: [u8; 32]) -> Self {
        let key_pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(
            &seed,
        )
        .expect("This should never fail, as the seed is exactly 32 bytes");
        Self { seed, key_pair }
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
    pub fn from_rng(mut rng: &mut dyn Crng) -> Self {
        Self::from_seed_owned(rng.gen_bytes())
    }

    /// Convert the current `ed25519::KeyPair` into an [`rcgen::KeyPair`].
    pub fn to_rcgen(&self) -> rcgen::KeyPair {
        let pkcs8_bytes = self.serialize_pkcs8_der();
        rcgen::KeyPair::try_from(pkcs8_bytes.as_slice()).expect(
            "Deserializing a freshly serialized \
                ed25519 key pair should never fail",
        )
    }

    /// Convert the current `ed25519::KeyPair` into a
    /// [`ring::signature::Ed25519KeyPair`].
    ///
    /// Requires a small intermediate serialization step since [`ring`] key
    /// pairs can't be cloned.
    pub fn to_ring(&self) -> ring::signature::Ed25519KeyPair {
        let pkcs8_bytes = self.serialize_pkcs8_der();
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

    /// Serialize the `ed25519::KeyPair` into PKCS#8 DER bytes.
    pub fn serialize_pkcs8_der(&self) -> [u8; PKCS_LEN] {
        serialize_keypair_pkcs8_der(&self.seed, self.public_key().as_inner())
    }

    /// Deserialize an `ed25519::KeyPair` from PKCS#8 DER bytes.
    pub fn deserialize_pkcs8_der(bytes: &[u8]) -> Result<Self, Error> {
        let (seed, expected_pubkey) = deserialize_keypair_pkcs8_der(bytes)
            .ok_or(Error::KeyDeserializeError)?;
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
    /// [`ed25519::verify_signed_struct`]
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

impl FromHex for KeyPair {
    fn from_hex(s: &str) -> Result<Self, hex::DecodeError> {
        <[u8; 32]>::from_hex(s).map(Self::from_seed_owned)
    }
}

impl FromStr for KeyPair {
    type Err = hex::DecodeError;
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
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
        const_utils::const_ref_cast(bytes)
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

    /// Serialize to the DER-encoded X.509 SubjectPublicKeyInfo struct.
    ///
    /// Adapted from [`rcgen::KeyPair::public_key_der`].
    ///
    /// [RFC 5280 section 4.1](https://tools.ietf.org/html/rfc5280#section-4.1)
    ///
    /// ```not_rust
    /// SubjectPublicKeyInfo  ::=  SEQUENCE  {
    ///     algorithm            AlgorithmIdentifier,
    ///     subjectPublicKey     BIT STRING
    /// }
    /// AlgorithmIdentifier  ::=  SEQUENCE  {
    ///     algorithm               OBJECT IDENTIFIER,
    ///     parameters              ANY DEFINED BY algorithm OPTIONAL
    /// }
    /// ```
    pub fn serialize_spki_der(&self) -> Vec<u8> {
        // TODO(max): Nerd bait: Optimize like `serialize_keypair_pkcs8_der`
        // below, which should allow us to remove the `yasna` dependency.
        // The current impl can be the reference impl for differential fuzzing.
        yasna::construct_der(|writer: yasna::DERWriter<'_>| {
            // `spki: SubjectPublicKeyInfo`
            writer.write_sequence(|writer| {
                // `algorithm: AlgorithmIdentifier` (id-Ed25519)
                writer.next().write_sequence(|writer| {
                    // `algorithm: OBJECT IDENTIFIER`
                    let oid =
                        ObjectIdentifier::from_slice(ed25519::PKCS_OID_SLICE);
                    writer.next().write_oid(&oid);
                    // `parameters: ANY DEFINED BY algorithm OPTIONAL` (none)
                });

                // `subjectPublicKey: BIT STRING`
                writer.next().write_bitvec_bytes(
                    self.as_slice(),
                    // 32 byte pubkey, 8 bits per byte
                    32 * 8,
                );
            })
        })
    }

    /// Deserialize from the DER-encoded X.509 SubjectPublicKeyInfo struct.
    ///
    /// [RFC 5280 section 4.1](https://tools.ietf.org/html/rfc5280#section-4.1)
    ///
    /// ```not_rust
    /// SubjectPublicKeyInfo  ::=  SEQUENCE  {
    ///     algorithm            AlgorithmIdentifier,
    ///     subjectPublicKey     BIT STRING
    /// }
    /// AlgorithmIdentifier  ::=  SEQUENCE  {
    ///     algorithm               OBJECT IDENTIFIER,
    ///     parameters              ANY DEFINED BY algorithm OPTIONAL
    /// }
    /// ```
    pub fn deserialize_spki_der(data: &[u8]) -> Result<Self, ASN1Error> {
        // TODO(max): Nerd bait: Optimize like `deserialize_keypair_pkcs8_der`
        // below, which should allow us to remove the `yasna` dependency.
        // The current impl can be the reference impl for differential fuzzing.
        yasna::parse_der(data, |reader| {
            // `spki: SubjectPublicKeyInfo`
            reader.read_sequence(|reader| {
                // `algorithm: AlgorithmIdentifier` (id-Ed25519)
                reader.next().read_sequence(|reader| {
                    // `algorithm: OBJECT IDENTIFIER`
                    let actual_oid = reader.next().read_oid()?;
                    let expected_oid =
                        ObjectIdentifier::from_slice(ed25519::PKCS_OID_SLICE);
                    if actual_oid != expected_oid {
                        return Err(ASN1Error::new(ASN1ErrorKind::Invalid));
                    }

                    // `parameters: ANY DEFINED BY algorithm OPTIONAL` (none)
                    Ok(())
                })?;

                // `subjectPublicKey: BIT STRING`
                let (pubkey_bytes, _num_bits) =
                    reader.next().read_bitvec_bytes()?;
                Self::try_from(pubkey_bytes.as_slice())
                    .map_err(|_| ASN1Error::new(ASN1ErrorKind::Invalid))
            })
        })
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

    /// Like [`ed25519::verify_signed_struct`] but only allows signatures
    /// produced by this `ed25519::PublicKey`.
    pub fn verify_self_signed_struct<'msg, T: Signable + Deserialize<'msg>>(
        &self,
        serialized: &'msg [u8],
    ) -> Result<Signed<T>, Error> {
        let accept_self_signer = |signer| signer == self;
        verify_signed_struct(accept_self_signer, serialized)
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

impl FromStr for PublicKey {
    type Err = hex::DecodeError;
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
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
        const_utils::const_ref_cast(sig)
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

impl FromStr for Signature {
    type Err = hex::DecodeError;
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
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

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impls {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for KeyPair {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>()
                .prop_map(|seed| Self::from_seed(&seed))
                .boxed()
        }
    }
}

// -- low-level PKCS#8 keypair serialization/deserialization -- //

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
const_utils::const_assert_usize_eq!(PKCS_LEN, 85);

/// Formats a key pair as `prefix || key || middle || pk`, where `prefix`
/// and `middle` are two pre-computed blobs.
///
/// Note: adapted from `ring`, which doesn't let you serialize as pkcs#8 via
/// any public API...
fn serialize_keypair_pkcs8_der(
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
fn deserialize_keypair_pkcs8_der(
    bytes: &[u8],
) -> Option<(&[u8; 32], &[u8; 32])> {
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
    use proptest::{
        arbitrary::any, prop_assert_eq, prop_assume, proptest,
        strategy::Strategy,
    };
    use proptest_derive::Arbitrary;

    use super::*;
    use crate::array;

    #[derive(Arbitrary, Serialize, Deserialize)]
    struct SignableBytes(Vec<u8>);

    impl Signable for SignableBytes {
        const DOMAIN_SEPARATOR: [u8; 32] =
            array::pad(*b"LEXE-REALM::SignableBytes");
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
        proptest!(|(seed in any::<[u8; 32]>())| {
            let key_pair1 = KeyPair::from_seed(&seed);
            let key_pair_bytes = key_pair1.serialize_pkcs8_der();
            let key_pair2 =
                KeyPair::deserialize_pkcs8_der(key_pair_bytes.as_slice())
                    .unwrap();

            assert_eq!(key_pair1.secret_key(), key_pair2.secret_key());
            assert_eq!(key_pair1.public_key(), key_pair2.public_key());
        });
    }

    /// [`KeyPair`] -> [`rcgen::KeyPair`] -> DER bytes -> [`KeyPair`]
    #[test]
    fn test_rcgen_der_roundtrip() {
        proptest!(|(seed in any::<[u8;32]>())| {
            let key_pair1 = KeyPair::from_seed(&seed);
            let rcgen_key_pair = key_pair1.to_rcgen();
            let key_der = rcgen_key_pair.serialize_der();
            let key_pair2 = KeyPair::deserialize_pkcs8_der(&key_der).unwrap();
            prop_assert_eq!(key_pair1.secret_key(), key_pair2.secret_key());
            prop_assert_eq!(key_pair1.public_key(), key_pair2.public_key());
        });
    }

    #[test]
    fn test_deserialize_pkcs8_different_lengths() {
        for size in 0..=256 {
            let bytes = vec![0x42_u8; size];
            let _ = deserialize_keypair_pkcs8_der(&bytes);
        }
    }

    #[test]
    fn test_to_rcgen() {
        proptest!(|(key_pair in any::<KeyPair>())| {
            let rcgen_key_pair = key_pair.to_rcgen();
            assert_eq!(
                rcgen_key_pair.public_key_raw(),
                key_pair.public_key().as_slice()
            );
            assert!(rcgen_key_pair.is_compatible(&rcgen::PKCS_ED25519));
        });
    }

    #[test]
    fn test_pubkey_spki_der_roundtrip() {
        proptest!(|(key_pair in any::<KeyPair>())| {
            let pubkey1 = key_pair.public_key();
            let keypair_rcgen = key_pair.to_rcgen();
            let pubkey_der_rcgen = keypair_rcgen.public_key_der();
            let pubkey_der_lexe = pubkey1.serialize_spki_der();

            // Ensure our DER encodings are equivalent to rcgen's.
            prop_assert_eq!(&pubkey_der_rcgen, &pubkey_der_lexe);

            // Ensure deserialization recovers the pubkey.
            let pubkey2 =
                ed25519::PublicKey::deserialize_spki_der(&pubkey_der_rcgen)
                    .unwrap();
            prop_assert_eq!(pubkey1, &pubkey2);
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
        proptest!(|(
            key_pair in any::<KeyPair>(),
            msg in any::<SignableBytes>()
        )| {
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
            key_pair in any::<KeyPair>(),
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
            key_pair in any::<KeyPair>(),
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
        proptest!(|(key_pair in any::<KeyPair>(), msg in any::<Vec<u8>>())| {
            let pubkey = key_pair.public_key();

            let sig = key_pair.sign_raw(&msg);
            pubkey.verify_raw(&msg, &sig).unwrap();
        });
    }

    #[test]
    fn test_sign_verify_struct() {
        #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
        struct Foo(u32);

        impl Signable for Foo {
            const DOMAIN_SEPARATOR: [u8; 32] = array::pad(*b"LEXE-REALM::Foo");
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct Bar(u32);

        impl Signable for Bar {
            const DOMAIN_SEPARATOR: [u8; 32] = array::pad(*b"LEXE-REALM::Bar");
        }

        fn arb_foo() -> impl Strategy<Value = Foo> {
            any::<u32>().prop_map(Foo)
        }

        proptest!(|(key_pair in any::<KeyPair>(), foo in arb_foo())| {
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
