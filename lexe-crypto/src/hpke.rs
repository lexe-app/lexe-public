//! Hybrid Public Key Encryption ([RFC 9180]), single-shot base mode.
//!
//! - Suite: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305.
//! - Sealed bytes: `enc || ct`, as in the RFC.
//! - The `hpke` crate's AEAD API returns the tag separately, so internally `ct`
//!   is `ciphertext || tag`.
//!
//! [RFC 9180]: https://www.rfc-editor.org/rfc/rfc9180.html

use std::{convert::Infallible, fmt};

use hpke::{
    Deserializable, Kem as _, OpModeR, OpModeS, Serializable,
    aead::{AeadTag, ChaCha20Poly1305},
    inout::InOutBuf,
    kdf::HkdfSha256,
    kem::X25519HkdfSha256,
};
use lexe_byte_array::ByteArray;
use zeroize::Zeroize;

use crate::rng::{Crng, RngExt};

/// The length in bytes of a ChaCha20-Poly1305 tag.
const TAG_LEN: usize = 16;

/// 48 B. The added overhead for a sealed message, on top of the plaintext
/// size: the encapsulated key plus the AEAD tag.
pub const SEAL_OVERHEAD: usize = PublicKey::LEN + TAG_LEN;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// Key encapsulation failed, e.g. the recipient key is a low-order point.
    Seal,
    /// The sealed bytes are shorter than [`SEAL_OVERHEAD`].
    TooShort,
    /// Decryption failed: wrong key, `info`, `aad`, or tampered ciphertext.
    Open,
}

/// An X25519 KEM public key.
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct PublicKey([u8; 32]);

lexe_byte_array::impl_byte_array!(PublicKey, 32);
lexe_byte_array::impl_fromstr_fromhex!(PublicKey, 32);
lexe_byte_array::impl_debug_display_as_hex!(PublicKey);
lexe_serde::impl_serde_hexstr_or_bytes!(PublicKey);

/// An X25519 KEM key pair. The private key is zeroized on drop.
pub struct KeyPair {
    sk: <Kem as hpke::Kem>::PrivateKey,
    pk: PublicKey,
}

type Kem = X25519HkdfSha256;
type Kdf = HkdfSha256;
type Aead = ChaCha20Poly1305;
type EncappedKey = <Kem as hpke::Kem>::EncappedKey;

impl PublicKey {
    /// Seal `plaintext` to this key, returning `enc || ct`.
    pub fn seal(
        &self,
        rng: &mut impl Crng,
        info: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let mut sealed = Vec::with_capacity(plaintext.len() + SEAL_OVERHEAD);
        sealed.resize(Self::LEN, 0);
        sealed.extend_from_slice(plaintext);

        // sealed := [enc placeholder] || [plaintext]

        let buffer = InOutBuf::from(&mut sealed[Self::LEN..]);
        let (enc, tag) =
            hpke::single_shot_seal_inout_detached_with_rng::<Aead, Kdf, Kem>(
                &OpModeS::Base,
                &self.into_hpke(),
                info,
                buffer,
                aad,
                &mut RngAdapter(rng),
            )
            .map_err(|_| Error::Seal)?;
        enc.write_exact(&mut sealed[..Self::LEN]);
        sealed.extend_from_slice(&tag.to_bytes());

        // sealed := [enc] || [ciphertext] || [tag]

        Ok(sealed)
    }

    fn from_hpke(pk: &<Kem as hpke::Kem>::PublicKey) -> Self {
        let mut array = [0u8; Self::LEN];
        pk.write_exact(&mut array);
        Self(array)
    }

    fn into_hpke(self) -> <Kem as hpke::Kem>::PublicKey {
        <Kem as hpke::Kem>::PublicKey::from_bytes(&self.0)
            .expect("Length is always correct")
    }
}

impl KeyPair {
    /// Derive a key pair from a 32-byte seed, per RFC 9180 `DeriveKeyPair`.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let (sk, pk) = Kem::derive_keypair(seed);
        Self {
            sk,
            pk: PublicKey::from_hpke(&pk),
        }
    }

    /// Sample a fresh key pair, e.g. an ephemeral one for a single request.
    pub fn from_rng(rng: &mut impl Crng) -> Self {
        let mut seed = rng.gen_bytes();
        let key_pair = Self::from_seed(&seed);
        seed.zeroize();
        key_pair
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.pk
    }

    /// Open `enc || ct` sealed to this key pair's public key.
    pub fn open(
        &self,
        info: &[u8],
        aad: &[u8],
        sealed: &[u8],
    ) -> Result<Vec<u8>, Error> {
        // sealed := [enc] || [ciphertext] || [tag]

        let (enc, rest) = sealed
            .split_at_checked(PublicKey::LEN)
            .ok_or(Error::TooShort)?;
        let tag_offset =
            rest.len().checked_sub(TAG_LEN).ok_or(Error::TooShort)?;
        let (ciphertext, tag) = rest.split_at(tag_offset);
        let enc = EncappedKey::from_bytes(enc).map_err(|_| Error::Open)?;
        let tag = AeadTag::<Aead>::from_bytes(tag).map_err(|_| Error::Open)?;

        let mut plaintext = ciphertext.to_vec();
        let buffer = InOutBuf::from(plaintext.as_mut_slice());
        hpke::single_shot_open_inout_detached::<Aead, Kdf, Kem>(
            &OpModeR::Base,
            &self.sk,
            &enc,
            info,
            buffer,
            aad,
            &tag,
        )
        .map_err(|_| Error::Open)?;

        // plaintext := [plaintext]

        Ok(plaintext)
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("hpke::KeyPair")
            .field("sk", &"..")
            .field("pk", &self.pk)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Seal => f.write_str("HPKE key encapsulation failed"),
            Self::TooShort => f.write_str("HPKE sealed bytes too short"),
            Self::Open => f.write_str("HPKE decryption failed"),
        }
    }
}

impl std::error::Error for Error {}

/// Adapts our `rand_core` 0.6 [`Crng`] to the `rand_core` 0.10 traits that
/// the `hpke` crate takes.
struct RngAdapter<'a, R>(&'a mut R);

impl<R: Crng> hpke::rand_core::TryRng for RngAdapter<'_, R> {
    type Error = Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok(self.0.next_u32())
    }
    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(self.0.next_u64())
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        self.0.fill_bytes(dst);
        Ok(())
    }
}

impl<R: Crng> hpke::rand_core::TryCryptoRng for RngAdapter<'_, R> {}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{Arbitrary, any},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::rng::FastRng;

    impl Arbitrary for PublicKey {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<FastRng>()
                .prop_map(|mut rng| *KeyPair::from_rng(&mut rng).public_key())
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use lexe_hex::hex;
    use proptest::{
        arbitrary::any, collection::vec, prop_assert, prop_assert_eq, proptest,
    };

    use super::*;
    use crate::rng::{FastRng, FixedRng};

    /// RFC 9180 Appendix A.2.1: our suite in base mode.
    #[test]
    fn rfc9180_a2_1_base_vector() {
        let ikm_e = hex::decode(
            "909a9b35d3dc4713a5e72a4da274b55d3d3821a37e5d099e74a647db583a904b",
        )
        .unwrap();
        let ikm_r = hex::decode(
            "1ac01f181fdf9f352797655161c58b75c656a6cc2716dcb66372da835542e1df",
        )
        .unwrap();
        let pk_r = PublicKey::from_str(
            "4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a",
        )
        .unwrap();
        let info =
            hex::decode("4f6465206f6e2061204772656369616e2055726e").unwrap();
        let aad = hex::decode("436f756e742d30").unwrap();
        let plaintext = hex::decode(
            "4265617574792069732074727574682c20747275746820626561757479",
        )
        .unwrap();
        let enc =
            "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a";
        let ciphertext = "1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db21993c62ce81883d2dd1b51a28";
        let expected = hex::decode(&format!("{enc}{ciphertext}")).unwrap();

        let key_pair = KeyPair::from_seed(&ikm_r.try_into().unwrap());
        assert_eq!(key_pair.public_key(), &pk_r);

        let mut rng = FixedRng(ikm_e);
        let sealed = pk_r.seal(&mut rng, &info, &aad, &plaintext).unwrap();
        assert_eq!(sealed, expected);
        assert_eq!(key_pair.open(&info, &aad, &sealed).unwrap(), plaintext);
    }

    #[test]
    fn seal_open_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            info in vec(any::<u8>(), 0..=64),
            aad in vec(any::<u8>(), 0..=64),
            plaintext in vec(any::<u8>(), 0..=256),
        )| {
            let key_pair = KeyPair::from_rng(&mut rng);
            let pk = key_pair.public_key();

            let sealed = pk.seal(&mut rng, &info, &aad, &plaintext).unwrap();
            prop_assert_eq!(sealed.len(), plaintext.len() + SEAL_OVERHEAD);
            prop_assert_eq!(&key_pair.open(&info, &aad, &sealed).unwrap(), &plaintext);

            // A fresh encapsulation makes every seal unique.
            let sealed2 = pk.seal(&mut rng, &info, &aad, &plaintext).unwrap();
            prop_assert!(sealed != sealed2);
        });
    }

    /// The untouched blob opens; any wrong key, `info`, `aad`, truncation,
    /// or bit flip fails to.
    #[test]
    fn reject_mismatch_truncation_and_tamper() {
        let cfg = proptest::test_runner::Config::with_cases(50);
        proptest!(cfg, |(
            mut rng in any::<FastRng>(),
            plaintext in vec(any::<u8>(), 0..=64),
            flip_idx in any::<proptest::sample::Index>(),
        )| {
            let key_pair = KeyPair::from_rng(&mut rng);
            let other = KeyPair::from_rng(&mut rng);
            let (info, aad) = (b"info", b"aad");
            let sealed = key_pair
                .public_key()
                .seal(&mut rng, info, aad, &plaintext)
                .unwrap();
            // Control: the untouched blob opens.
            prop_assert_eq!(key_pair.open(info, aad, &sealed), Ok(plaintext.clone()));

            prop_assert_eq!(other.open(info, aad, &sealed), Err(Error::Open));
            prop_assert_eq!(key_pair.open(b"other", aad, &sealed), Err(Error::Open));
            prop_assert_eq!(key_pair.open(info, b"other", &sealed), Err(Error::Open));

            for len in 0..sealed.len() {
                let expected = if len < SEAL_OVERHEAD {
                    Error::TooShort
                } else {
                    Error::Open
                };
                prop_assert_eq!(key_pair.open(info, aad, &sealed[..len]), Err(expected));
            }

            let mut tampered = sealed.clone();
            tampered[flip_idx.index(sealed.len())] ^= 1;
            prop_assert_eq!(key_pair.open(info, aad, &tampered), Err(Error::Open));
        });
    }

    #[test]
    fn pubkey_serde_roundtrip() {
        proptest!(|(pk in any::<PublicKey>())| {
            prop_assert_eq!(pk.to_string().parse::<PublicKey>().unwrap(), pk);
            let json = serde_json::to_string(&pk).unwrap();
            prop_assert_eq!(serde_json::from_str::<PublicKey>(&json).unwrap(), pk);
            let bcs = bcs::to_bytes(&pk).unwrap();
            prop_assert_eq!(bcs::from_bytes::<PublicKey>(&bcs).unwrap(), pk);
        });
    }

    /// Sealing to a low-order point fails instead of yielding a weak secret.
    #[test]
    fn reject_low_order_recipient() {
        let mut rng = FastRng::from_u64(1);
        let low_order = PublicKey::from_array([0; PublicKey::LEN]);
        assert_eq!(low_order.seal(&mut rng, b"", b"", b""), Err(Error::Seal));
    }
}
