//! Securely encrypt and decrypted blobs, usually for remote storage.
//!
//! ## Design Considerations
//!
//! * AES-256-GCM uses 12-byte nonces (2^96 bits).
//! * For a given key, any nonce reuse is catastrophic w/ AES-GCM.
//! * Synthetic nonce / nonce reuse resistant schemes like AES-SIV-GCM aren't
//!   available in [`ring`] or have undesirable properties (multiple passes, max
//!   2^32 encryptions).
//! * [`ring`] doesn't support XChaCha20-Poly1305, which would let us use a
//!   larger nonce.
//! * We need to use [`ring`] b/c TLS. We don't want to depend on other crypto
//!   libraries b/c attack surface and binary bloat.
//! * For our particular use case, we don't particularly care about
//!   single-message (key, nonce) wear-out, since our messages aren't
//!   particularly large (at most a few MiB).
//! * We also don't have access to a consistent monotonic counter b/c
//!   distributed system and adversarial host, so we use random nonces.
//! * If we had a reliable counter, we could safely encrypt ~2^64 messages
//!   before key wear-out, which would be sufficient for us to simplify and just
//!   use one key for all encryptions.
//! * For a given key and perfectly random nonces, with nonce collision
//!   probability = 2^-32 (standard NIST bound), we can expect key wear-out
//!   after 2^32 encryptions.
//!
//! ## Design
//!
//! This scheme is inspired by "Derive Key Mode" described in
//! [(2017) GueronLindel](https://eprint.iacr.org/2017/702.pdf).
//! "Derive Key Mode" uses a long-term "master key" (see `AesMasterKey`), which
//! isn't used to encrypt data; rather, it's used to derive per-message keys
//! from a large random key-id, sampled per message (see `KeyId`).
//!
//! In our case, we use a 32-byte (2^256 bit) key id to derive each per-message
//! `EncryptKey`/`DecryptKey`, which gives us plenty of breathing room as far as
//! safety bounds are concerned.
//!
//! For the AAD, taking a single `&[u8]` would require the caller to allocate
//! and canonically serialize (length-prefixes, etc...) when there are multiple
//! things to bind. Then, we would need to copy+allocate again in order to bind
//! the `version`, `key-id`, and user AAD. To avoid this the user passes the AAD
//! as a list of segments (like fields of a struct). For more info, see `Aad`.
//!
//! We use an AES-256-GCM nonce of all zeroes, since keys are single-use and
//! 256 bits of security are Good Enough^tm.
//!
//! The scheme in simplified pseudo-code, encryption only:
//!
//! ```text
//! master-key := (secret derived from user's root seed)
//!
//! Aad(version, key-id, user-aad: &[&[u8]]) :=
//! 1. return bcs::to_bytes({ version, key-id, user-aad })
//!
//! Encrypt(master-key, user-aad: &[&[u8]], plaintext) :=
//! 1. version := 0_u8
//! 2. key-id := random 32-byte value
//! 3. aad := Aad(version, key-id, user-aad)
//! 4. encrypt-key := HKDF-Extract-Expand(
//!         ikm=master-key,
//!         salt=array::pad::<32>("LEXE-REALM::AesMasterKey"),
//!         info=key-id,
//!         out-len=32 bytes,
//!    )
//! 5. (ciphertext, tag) := AES-256-GCM(encrypt-key, nonce=[0; 12], aad, plaintext)
//! 6. output := version || key-id || ciphertext || tag
//! 7. return output
//! ```
//!
//! ## References
//!
//! * [(2017) GueronLindel](https://eprint.iacr.org/2017/702.pdf) ([video](https://www.youtube.com/watch?v=WEJ451rmhk4))
//!
//! This paper, "Better Bounds for Block Cipher Modes of Operation via
//! Nonce-Based Key Derivation", shows how "Derive Key Mode" significantly
//! improves the security bounds over the standard long-lived key approach.
//!
//! * [(2020) Cryptographic Wear-out for Symmetric Encryption](https://soatok.blog/2020/12/24/cryptographic-wear-out-for-symmetric-encryption/)
//!
//! This article describes symmetric security bounds nicely.

use std::fmt;

use bytes::BufMut;
use lexe_std::array;
use ref_cast::RefCast;
use ring::{
    aead::{self, BoundKey},
    hkdf,
};
use serde::Serialize;
use thiserror::Error;

use crate::rng::{Crng, RngExt};

/// serialized version length
const VERSION_LEN: usize = 1;

/// serialized [`KeyId`] length
const KEY_ID_LEN: usize = 32;

/// serialized AES-256-GCM tag length
const TAG_LEN: usize = 16;

/// The length of the final encrypted ciphertext + version byte + key_id + tag
/// given an input plaintext length.
pub const fn encrypted_len(plaintext_len: usize) -> usize {
    VERSION_LEN + KEY_ID_LEN + plaintext_len + TAG_LEN
}

/// The `AesMasterKey` is used to derive unique single-use encrypt keys for
/// encrypting or decrypting a blob.
///
/// `RootSeed` -- derive("vfs master key") --> `AesMasterKey`
// We store the salted+extracted PRK directly to avoid recomputing it every
// time we encrypt something.
pub struct AesMasterKey(hkdf::Prk);

/// `KeyId` is the value used to derive the single-use message
/// encryption/decryption key from the [`AesMasterKey`] HKDF.
///
/// As explained in the module docs, AES-GCM nonces are too small (12-bytes), so
/// we use what is effectively a synthetic nonce scheme by deriving single-use
/// keys from a larger pool of entropy (2^32 bits) for each separate encryption.
#[derive(RefCast, Serialize)]
#[repr(transparent)]
struct KeyId([u8; 32]);

/// `Aad` is canonically serialized and then passed to AES-256-GCM as the `aad`
/// (additional authenticated data) parameter.
///
/// It serves to:
///
/// 1. bind the protocol version
/// 2. bind the encryption key (via the key id)
/// 3. bind the user-provided additional authenticated data segments, including
///    the number of segments, and the lengths of each segment.
#[derive(Serialize)]
struct Aad<'data, 'aad> {
    version: u8,
    key_id: &'data KeyId,
    aad: &'aad [&'aad [u8]],
}

struct EncryptKey(aead::SealingKey<ZeroNonce>);

struct DecryptKey(aead::OpeningKey<ZeroNonce>);

/// A single-use, all-zero nonce that panics if used to encrypt or decrypt data
/// more than once (for a particular instance).
struct ZeroNonce(Option<aead::Nonce>);

#[derive(Clone, Debug, Error)]
#[error("decrypt error: ciphertext or metadata may be corrupted")]
pub struct DecryptError;

impl fmt::Debug for AesMasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AesMasterKey(..)")
    }
}

impl AesMasterKey {
    const HKDF_SALT: [u8; 32] = array::pad(*b"LEXE-REALM::AesMasterKey");

    pub fn new(root_seed_derived_secret: &[u8; 32]) -> Self {
        Self(
            hkdf::Salt::new(hkdf::HKDF_SHA256, &Self::HKDF_SALT)
                .extract(root_seed_derived_secret),
        )
    }

    fn derive_unbound_key(&self, key_id: &KeyId) -> aead::UnboundKey {
        aead::UnboundKey::from(
            self.0
                .expand(&[key_id.as_slice()], &aead::AES_256_GCM)
                .expect("This should never fail"),
        )
    }

    fn derive_encrypt_key(&self, key_id: &KeyId) -> EncryptKey {
        let nonce = ZeroNonce::new();
        let key = aead::SealingKey::new(self.derive_unbound_key(key_id), nonce);
        EncryptKey(key)
    }

    fn derive_decrypt_key(&self, key_id: &KeyId) -> DecryptKey {
        let nonce = ZeroNonce::new();
        let key = aead::OpeningKey::new(self.derive_unbound_key(key_id), nonce);
        DecryptKey(key)
    }

    pub fn encrypt<R: Crng>(
        &self,
        rng: &mut R,
        aad: &[&[u8]],
        // A size hint so we can possibly avoid reallocing. If you don't know
        // how long the plaintext will be, just set this to None.
        data_size_hint: Option<usize>,
        // This closure should write the object to the provided &mut Vec<u8>.
        // See tests as well as node / lsp `encrypt_*` for examples.
        write_data_cb: &dyn Fn(&mut Vec<u8>),
    ) -> Vec<u8> {
        let version = 0;
        let key_id = KeyId::from_rng(rng);

        let aad = Aad {
            version,
            key_id: &key_id,
            aad,
        }
        .serialize();

        // reserve enough capacity for at least version, key_id, and tag
        let approx_encrypted_len = encrypted_len(data_size_hint.unwrap_or(0));
        let mut data = Vec::with_capacity(approx_encrypted_len);

        // data := ""

        data.put_u8(version);
        data.put(key_id.as_slice());
        let plaintext_offset = data.len();

        // data := [version] || [key_id]

        write_data_cb(&mut data);

        // data := [version] || [key_id] || [plaintext]

        self.derive_encrypt_key(&key_id).encrypt_in_place(
            aad.as_slice(),
            &mut data,
            plaintext_offset,
        );

        // data := [version] || [key_id] || [ciphertext] || [tag]

        data
    }

    pub fn decrypt(
        &self,
        aad: &[&[u8]],
        mut data: Vec<u8>,
    ) -> Result<Vec<u8>, DecryptError> {
        // data := [version] || [key_id] || [ciphertext] || [tag]

        const MIN_DATA_LEN: usize = encrypted_len(0 /* plaintext len */);
        if data.len() < MIN_DATA_LEN {
            return Err(DecryptError);
        }

        // parse out version and key_id w/o advancing `data`
        let (version, key_id) = {
            let (version, data) = data
                .split_first_chunk::<VERSION_LEN>()
                .expect("data.len() checked above");
            let (key_id, _) = data
                .split_first_chunk::<KEY_ID_LEN>()
                .expect("data.len() checked above");
            (version[0], key_id)
        };

        if version != 0 {
            return Err(DecryptError);
        }
        let key_id = KeyId::from_ref(key_id);
        let decrypt_key = self.derive_decrypt_key(key_id);

        let aad = Aad {
            version,
            key_id,
            aad,
        }
        .serialize();

        let ciphertext_and_tag_offset = VERSION_LEN + KEY_ID_LEN;
        decrypt_key.decrypt_in_place(
            &aad,
            &mut data,
            ciphertext_and_tag_offset,
        )?;

        // data := [plaintext]

        Ok(data)
    }
}

impl EncryptKey {
    // aad := additional authenticated data (e.g. protocol transcripts)
    // data := [version] || [key_id] || [plaintext]
    // plaintext_offset := starting index of `[plaintext]` in `data`
    fn encrypt_in_place(
        mut self,
        aad: &[u8],
        data: &mut Vec<u8>,
        plaintext_offset: usize,
    ) {
        assert!(plaintext_offset <= data.len());

        let aad = aead::Aad::from(aad);
        let tag = self
            .0
            .seal_in_place_separate_tag(aad, &mut data[plaintext_offset..])
            .expect(
                "Cannot encrypt more than ~4 GiB at once (should never happen)",
            );
        data.extend_from_slice(tag.as_ref());
    }
}

impl DecryptKey {
    // aad := additional authenticated data (e.g. protocol transcripts)
    // data := [version] || [key_id] || [ciphertext] || [tag]
    // ciphertext_and_tag_offset := starting index of `[ciphertext] || [tag]`
    fn decrypt_in_place(
        mut self,
        aad: &[u8],
        data: &mut Vec<u8>,
        ciphertext_and_tag_offset: usize,
    ) -> Result<(), DecryptError> {
        // `open_within` will shift the decrypted plaintext to the start of
        // `data`.
        let aad = aead::Aad::from(aad);

        let plaintext_ref = self
            .0
            .open_within(aad, data, ciphertext_and_tag_offset..)
            .map_err(|_| DecryptError)?;
        let plaintext_len = plaintext_ref.len();

        // decrypting happens in-place. set the length of the now decrypted
        // plaintext blob.
        data.truncate(plaintext_len);

        Ok(())
    }
}

impl KeyId {
    #[inline]
    const fn from_ref(arr: &[u8; 32]) -> &Self {
        const_utils::const_ref_cast(arr)
    }

    #[inline]
    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    fn from_rng<R: Crng>(rng: &mut R) -> Self {
        Self(rng.gen_bytes())
    }
}

impl Aad<'_, '_> {
    fn serialize(&self) -> Vec<u8> {
        let len = bcs::serialized_size(self)
            .expect("Serializing the AAD should never fail");

        let mut out = Vec::with_capacity(len);
        bcs::serialize_into(&mut out, self)
            .expect("Serializing the AAD should never fail");
        out
    }
}

impl ZeroNonce {
    fn new() -> Self {
        Self(Some(aead::Nonce::assume_unique_for_key([0u8; 12])))
    }
}

impl aead::NonceSequence for ZeroNonce {
    fn advance(&mut self) -> Result<aead::Nonce, ring::error::Unspecified> {
        Ok(self.0.take().expect(
            "We somehow encrypted / decrypted more than once with the same key",
        ))
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::root_seed::RootSeed;

    impl Arbitrary for AesMasterKey {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<RootSeed>()
                .prop_map(|seed| seed.derive_vfs_master_key())
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::{
        arbitrary::any, collection::vec, prop_assert, prop_assert_eq, proptest,
    };

    use super::*;
    use crate::{rng::FastRng, root_seed::RootSeed};

    #[test]
    fn test_aad_compat() {
        let aad = Aad {
            version: 0,
            key_id: KeyId::from_ref(&[0x69; 32]),
            aad: &[],
        }
        .serialize();

        let expected_aad = hex::decode(
            "00\
             6969696969696969696969696969696969696969696969696969696969696969\
             00",
        )
        .unwrap();

        assert_eq!(&aad, &expected_aad);

        let aad = Aad {
            version: 0,
            key_id: KeyId::from_ref(&[0x42; 32]),
            aad: &[b"aaaaaaaa".as_slice(), b"0123456789".as_slice()],
        }
        .serialize();

        let expected_aad = hex::decode(
            "00\
             4242424242424242424242424242424242424242424242424242424242424242\
             02\
                08\
                    6161616161616161\
                0a\
                    30313233343536373839",
        )
        .unwrap();
        assert_eq!(&aad, &expected_aad);
    }

    #[test]
    fn test_decrypt_compat() {
        let mut rng = FastRng::from_u64(123);
        let root_seed = RootSeed::from_rng(&mut rng);
        let vfs_key = root_seed.derive_vfs_master_key();

        // aad = [], plaintext = ""

        // uncomment to regen
        // let encrypted = vfs_key.encrypt(&mut rng, &[], None, &|_| ());
        // println!("encrypted: {}", hex::display(&encrypted));

        let encrypted = hex::decode(
            // [version] || [key_id] || [ciphertext] || [tag]
            "00\
             b0abd2beab31c1d925c5d8059cf90068eece2c41a3a6e4454d84e36ad6858a01\
             \
             0e2d1f6d16e9bb5738de28b4f180f07f",
        )
        .unwrap();

        let decrypted = vfs_key.decrypt(&[], encrypted).unwrap();
        assert_eq!(decrypted.as_slice(), b"");

        // aad = ["my context"], plaintext = "my cool message"

        let aad = b"my context".as_slice();
        let plaintext = b"my cool message".as_slice();

        // // uncomment to regen
        // #[rustfmt::skip]
        // let encrypted = vfs_key
        //     .encrypt(&mut rng, &[aad], None, &|out| out.put(plaintext));
        // println!("encrypted: {}", hex::display(&encrypted));

        let encrypted = hex::decode(
            // [version] || [key_id] || [ciphertext] || [tag]
            "00\
             c87fea5c4db8c16d3dae5a6ead5ee5985fa7c38721b9624e37772adea6a48aae\
             22f52c6f08440092338d16e3402eaf\
             c3972d357e56dad4cc42c6a80da4ac35",
        )
        .unwrap();

        let decrypted = vfs_key.decrypt(&[aad], encrypted).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            aad in vec(vec(any::<u8>(), 0..=16), 0..=4),
            plaintext in vec(any::<u8>(), 0..=256),
        )| {
            let root_seed = RootSeed::from_rng(&mut rng);
            let vfs_key = root_seed.derive_vfs_master_key();

            let aad_ref = aad
                .iter()
                .map(|x| x.as_slice())
                .collect::<Vec<_>>();

            let encrypted = vfs_key.encrypt(&mut rng, &aad_ref, Some(plaintext.len()), &|out: &mut Vec<u8>| {
                out.extend_from_slice(&plaintext);
            });

            let decrypted = vfs_key.decrypt(&aad_ref, encrypted.clone()).unwrap();
            prop_assert_eq!(&plaintext, &decrypted);

            let encrypted2 = vfs_key.encrypt(&mut rng, &aad_ref, None, &|out: &mut Vec<u8>| {
                out.extend_from_slice(&plaintext);
            });

            prop_assert!(encrypted != encrypted2);
        });
    }
}
