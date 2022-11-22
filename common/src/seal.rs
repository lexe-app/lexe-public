//! Module for sealing and unsealing blobs to/from remote storage.

use bytes::{BufMut, BytesMut};
use ref_cast::RefCast;
use ring::aead::{self, BoundKey};
use ring::hkdf;
use thiserror::Error;

use crate::rng::Crng;
use crate::{const_ref_cast, sha256};

/// serialized version length
const VERSION_LEN: usize = 1;

/// serialized [`KeyId`] length
const KEY_ID_LEN: usize = 32;

/// serialized AES-256-GCM tag length
const TAG_LEN: usize = 16;

/// The length of an encrypted ciphertext + header + tag given an input
/// plaintext length.
const fn sealed_len(plaintext_len: usize) -> usize {
    VERSION_LEN + KEY_ID_LEN + plaintext_len + TAG_LEN
}

/// The `VfsKey` is used to derive unique single-use seal keys for encrypting
/// a single blob.
///
/// RootSeed -- derive "vfs" --> VfsKey
// store the salted+extracted PRK directly to avoid recomputing it every
// time we seal something.
pub struct VfsKey(hkdf::Prk);

#[derive(RefCast)]
#[repr(transparent)]
pub struct KeyId([u8; 32]);

pub struct SealKey(aead::SealingKey<ZeroNonce>);

pub struct UnsealKey(aead::OpeningKey<ZeroNonce>);

/// A nonce wrapper that panics if a nonce is used to seal/unseal more than once
struct ZeroNonce(Option<aead::Nonce>);

#[derive(Debug, Error)]
#[error("unseal error: ciphertext or metadata may be corrupted")]
pub struct UnsealError;

impl VfsKey {
    const HKDF_SALT: [u8; 32] =
        sha256::digest_const(b"LEXE-REALM::VfsKey").into_inner();

    pub fn new(seed_derived_secret: &[u8; 32]) -> Self {
        Self(
            hkdf::Salt::new(hkdf::HKDF_SHA256, &Self::HKDF_SALT)
                .extract(seed_derived_secret),
        )
    }

    fn derive_unbound_key(&self, key_id: &KeyId) -> aead::UnboundKey {
        aead::UnboundKey::from(
            self.0
                .expand(&[key_id.as_slice()], &aead::AES_256_GCM)
                .expect("This should never fail"),
        )
    }

    fn derive_seal_key(&self, key_id: &KeyId) -> SealKey {
        let nonce = ZeroNonce::new();
        let key = aead::SealingKey::new(self.derive_unbound_key(key_id), nonce);
        SealKey(key)
    }

    fn derive_unseal_key(&self, key_id: &KeyId) -> UnsealKey {
        let nonce = ZeroNonce::new();
        let key = aead::OpeningKey::new(self.derive_unbound_key(key_id), nonce);
        UnsealKey(key)
    }

    pub fn seal<R: Crng>(
        &self,
        rng: &mut R,
        aad: &[u8],
        // A size hint so we can possibly avoid reallocing. If you don't know
        // how long the plaintext will be, just set this to None.
        data_size_hint: Option<usize>,
        write_data_cb: &dyn Fn(&mut BytesMut),
    ) -> BytesMut {
        let version = 0;
        let key_id = KeyId::gen(rng);

        // reserve enough capacity for at least version, key_id, and tag
        let approx_sealed_len = sealed_len(data_size_hint.unwrap_or(0));
        let mut data = BytesMut::with_capacity(approx_sealed_len);

        // data := ""

        data.put_u8(version);
        data.put(key_id.as_slice());
        let plaintext_offset = data.len();

        // data := [version] || [key_id]

        write_data_cb(&mut data);

        // data := [version] || [key_id] || [plaintext]

        self.derive_seal_key(&key_id).seal_in_place(
            aad,
            &mut data,
            plaintext_offset,
        );

        // data := [version] || [key_id] || [ciphertext] || [tag]

        data
    }

    pub fn unseal(
        &self,
        aad: &[u8],
        mut data: BytesMut,
    ) -> Result<BytesMut, UnsealError> {
        // data := [version] || [key_id] || [ciphertext] || [tag]

        const MIN_DATA_LEN: usize = sealed_len(0 /* plaintext len */);
        if data.len() < MIN_DATA_LEN {
            return Err(UnsealError);
        }

        // parse out version and key_id w/o advancing `data`
        let (version, key_id) = {
            let data = data.as_ref();
            let (version, data) = data.split_array_ref::<VERSION_LEN>();
            let (key_id, _) = data.split_array_ref::<KEY_ID_LEN>();
            (version, key_id)
        };

        if version != &[0] {
            return Err(UnsealError);
        }
        let key_id = KeyId::from_ref(key_id);
        let unseal_key = self.derive_unseal_key(key_id);

        let ciphertext_and_tag_offset = VERSION_LEN + KEY_ID_LEN;
        unseal_key.unseal_in_place(
            aad,
            &mut data,
            ciphertext_and_tag_offset,
        )?;

        // data := [plaintext]

        Ok(data)
    }
}

impl SealKey {
    // aad := additional authenticated data (e.g. protocol transcripts)
    // data := [version] || [key_id] || [plaintext]
    // plaintext_offset := starting index of `[plaintext]` in `data`
    fn seal_in_place(
        mut self,
        aad: &[u8],
        data: &mut BytesMut,
        plaintext_offset: usize,
    ) {
        assert!(plaintext_offset <= data.len());

        let aad = aead::Aad::from(aad);
        let tag = self
            .0
            .seal_in_place_separate_tag(aad, &mut data[plaintext_offset..])
            .expect(
                "Cannot seal more than ~4 GiB at once (should never happen)",
            );
        data.extend_from_slice(tag.as_ref());
    }
}

impl UnsealKey {
    // aad := additional authenticated data (e.g. protocol transcripts)
    // data := [version] || [key_id] || [ciphertext] || [tag]
    // ciphertext_and_tag_offset := starting index of `[ciphertext] || [tag]`
    fn unseal_in_place(
        mut self,
        aad: &[u8],
        data: &mut BytesMut,
        ciphertext_and_tag_offset: usize,
    ) -> Result<(), UnsealError> {
        // `open_within` will shift the decrypted plaintext to the start of
        // `data`.
        let aad = aead::Aad::from(aad);
        let plaintext_ref = self
            .0
            .open_within(aad, data, ciphertext_and_tag_offset..)
            .map_err(|_| UnsealError)?;
        let plaintext_len = plaintext_ref.len();

        // unsealing happens in-place. set the length of the now decrypted
        // plaintext blob.
        data.truncate(plaintext_len);

        Ok(())
    }
}

impl KeyId {
    #[inline]
    const fn from_ref(arr: &[u8; 32]) -> &Self {
        const_ref_cast(arr)
    }

    #[inline]
    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    fn gen<R: Crng>(rng: &mut R) -> Self {
        let mut key_id = KeyId([0u8; 32]);
        rng.fill_bytes(&mut key_id.0[..]);
        key_id
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
            "We somehow sealed / unseal more than once with the same key",
        ))
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::collection::vec;
    use proptest::{prop_assert, prop_assert_eq, proptest};

    use super::*;
    use crate::hex;
    use crate::rng::SmallRng;
    use crate::root_seed::RootSeed;

    #[test]
    fn test_unseal_compat() {
        let mut rng = SmallRng::from_u64(123);
        let root_seed = RootSeed::from_rng(&mut rng);
        let vfs_key = root_seed.derive_vfs_key();

        let aad = b"my context";
        let plaintext = b"my cool message";
        let sealed = hex::decode(
            "00b0abd2beab31c1d925c5d8059cf90068eece2c41a3a6e4454d84e36ad6858a01\
             583fd2d2df55114ad7c601726e1c8c120351d54130a6a1cc66acdd0a459813"
        ).unwrap();

        let unsealed = vfs_key
            .unseal(aad, BytesMut::from(sealed.as_slice()))
            .unwrap();

        assert_eq!(unsealed.as_ref(), plaintext.as_slice());
    }

    #[test]
    fn test_seal_unseal_roundtrip() {
        proptest!(|(
            mut rng in any::<SmallRng>(),
            aad in vec(any::<u8>(), 0..=64),
            plaintext in vec(any::<u8>(), 0..=256),
        )| {
            let root_seed = RootSeed::from_rng(&mut rng);
            let vfs_key = root_seed.derive_vfs_key();

            let sealed = vfs_key.seal(&mut rng, &aad, Some(plaintext.len()), &|out: &mut BytesMut| {
                out.extend_from_slice(&plaintext);
            });

            let unsealed = vfs_key.unseal(&aad, sealed.clone()).unwrap();
            prop_assert_eq!(&plaintext, &unsealed);

            let sealed2 = vfs_key.seal(&mut rng, &aad, None, &|out: &mut BytesMut| {
                out.extend_from_slice(&plaintext);
            });

            prop_assert!(sealed != sealed2);
        });
    }
}
