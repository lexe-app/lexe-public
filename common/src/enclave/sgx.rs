//! SGX-specific implementations for in-enclave APIs

use std::borrow::Cow;

use ring::aead::{
    Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey,
    AES_256_GCM,
};
use ring::hkdf::{self, HKDF_SHA256};
use secrecy::zeroize::Zeroizing;
use sgx_isa::{AttributesFlags, Keyname, Keypolicy, Report};

use crate::enclave::{Error, Sealed};
use crate::hex;
use crate::rng::Crng;

#[cfg(test)]
const HKDF_SALT_STR: &[u8] = b"LEXE-HASH-REALM::SgxSealing";

/// We salt the HKDF for domain separation purposes. The raw bytes here are
/// equal to the hash value: `SHA-256(b"LEXE-HASH-REALM::SgxSealing")`.
const HKDF_SALT: [u8; 32] = hex::decode_const(
    b"66331e89a9282101072c8879263a948ca8e48ef22c6f18eccf11d91864b3911a",
);

/// AES-256-GCM tag length
pub const TAG_LEN: usize = 16;

/// The length of an encrypted ciphertext given an input plaintext length.
pub const fn encrypted_len(plaintext_len: usize) -> usize {
    plaintext_len + TAG_LEN
}

/// The length of a decrypted plaintext given an input ciphertext length.
pub const fn decrypted_len(ciphertext_len: usize) -> usize {
    ciphertext_len - TAG_LEN
}

/// A convenience wrapper around an SGX [`sgx_isa::Keyrequest`].
struct KeyRequest(sgx_isa::Keyrequest);

impl KeyRequest {
    const UNPADDED_SIZE: usize = sgx_isa::Keyrequest::UNPADDED_SIZE; // 512

    /// The [`sgx_isa::Keyrequest`] struct size without the trailing reserved
    /// field.
    const TRUNCATED_SIZE: usize = 76; // == 512 - 436

    /// Generate a request for a unique, one-time-use sealing key. The sealing
    /// key will only be recoverable on enclaves with the same `MRENCLAVE`
    /// measurement.
    fn gen_sealing_request(rng: &mut dyn Crng, self_report: &Report) -> Self {
        let mut keyid = [0u8; 32];
        rng.fill_bytes(&mut keyid);

        // TODO(phlip9): take another pass at choosing attribute masks.

        // ignore reserved bits + PROVISIONKEY + EINITTOKENKEY
        let attribute_mask: u64 = !(0xffffffffffffc0
            | AttributesFlags::PROVISIONKEY.bits()
            | AttributesFlags::EINITTOKENKEY.bits());
        // bind all
        let xfrm_mask: u64 = !0;
        // bind upper byte
        let misc_mask: u32 = 0xf0000000;

        Self(sgx_isa::Keyrequest {
            keyname: Keyname::Seal as _,
            keypolicy: Keypolicy::MRENCLAVE,
            isvsvn: self_report.isvsvn,
            cpusvn: self_report.cpusvn,
            attributemask: [attribute_mask, xfrm_mask],
            miscmask: misc_mask,
            keyid,
            ..Default::default()
        })
    }

    /// Truncate a full 512 SGX [`Keyrequest`] to 76 bytes, leaving off the
    /// empty reserved bytes. This makes our `Sealed` data significantly
    /// more compact.
    fn as_bytes(&self) -> &[u8] {
        let bytes: &[u8] = self.0.as_ref();
        &bytes[0..Self::TRUNCATED_SIZE]
    }

    /// Deserialize a [`KeyRequest`] from bytes. Inputs must be between 76 B and
    /// 512 B.
    fn try_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let valid_size = Self::TRUNCATED_SIZE..=Self::UNPADDED_SIZE;
        if !valid_size.contains(&bytes.len()) {
            return Err(Error::Other);
        }

        let mut buf = [0u8; Self::UNPADDED_SIZE];
        buf[..bytes.len()].copy_from_slice(bytes);
        Ok(Self(
            sgx_isa::Keyrequest::try_copy_from(&buf)
                .expect("Should never fail"),
        ))
    }

    fn derive_key_material(&self) -> Result<Zeroizing<[u8; 16]>, Error> {
        Ok(Zeroizing::new(self.0.egetkey()?))
    }

    /// We sample a unique sealing key per seal request. just grab the random
    /// part of the label as a nonce.
    fn single_use_nonce(&self) -> OnlyOnce {
        let (_, nonce) = self.0.keyid.rsplit_array_ref::<12>();
        OnlyOnce(Some(Nonce::assume_unique_for_key(*nonce)))
    }

    // TODO(phlip9): will need wrapper types around SealingKey and OpeningKey to
    // successfully implement Zeroize...

    fn derive_sealing_key(
        &self,
        label: &[u8],
    ) -> Result<SealingKey<OnlyOnce>, Error> {
        let key_material = self.derive_key_material()?;
        Ok(SealingKey::new(
            derive_aesgcm_key(&key_material, label),
            self.single_use_nonce(),
        ))
    }

    fn derive_unsealing_key(
        &self,
        label: &[u8],
    ) -> Result<OpeningKey<OnlyOnce>, Error> {
        let key_material = self.derive_key_material()?;
        Ok(OpeningKey::new(
            derive_aesgcm_key(&key_material, label),
            self.single_use_nonce(),
        ))
    }
}

fn derive_aesgcm_key(key_material: &[u8; 16], label: &[u8]) -> UnboundKey {
    UnboundKey::from(
        hkdf::Salt::new(HKDF_SHA256, &HKDF_SALT)
            .extract(key_material.as_slice())
            .expand(&[label], &AES_256_GCM)
            .expect("Failed to derive sealing key from key material"),
    )
}

/// A nonce wrapper that panics if a `Nonce` is seal/unseal'ed more than once.
struct OnlyOnce(Option<Nonce>);

impl OnlyOnce {
    fn new(nonce: Nonce) -> Self {
        Self(Some(nonce))
    }
}

impl NonceSequence for OnlyOnce {
    fn advance(&mut self) -> Result<Nonce, ring::error::Unspecified> {
        Ok(self
            .0
            .take()
            .expect("sealed / unseal more than once with the same key"))
    }
}

pub fn seal(
    rng: &mut dyn Crng,
    label: &[u8],
    data: &[u8],
) -> Result<Sealed<'static>, Error> {
    // TODO(phlip9): put a more reasonable max length
    if data.len() > decrypted_len(usize::MAX) {
        return Err(Error::Other);
    }

    let self_report = Report::for_self();
    let keyrequest = KeyRequest::gen_sealing_request(rng, &self_report);
    let mut sealing_key = keyrequest.derive_sealing_key(label)?;

    // TODO(phlip9): allow sealing in place w/o allocating

    let mut ciphertext = vec![0u8; encrypted_len(data.len())];
    ciphertext[0..data.len()].copy_from_slice(data);

    let tag = sealing_key.seal_in_place_separate_tag(
        Aad::empty(),
        &mut ciphertext[0..data.len()],
    )?;
    ciphertext[data.len()..].copy_from_slice(tag.as_ref());

    Ok(Sealed {
        keyrequest: keyrequest.as_bytes().to_vec().into(),
        ciphertext: Cow::Owned(ciphertext),
    })
}

pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    // the ciphertext is too small
    if sealed.ciphertext.len() < TAG_LEN {
        return Err(Error::Other);
    }

    let keyrequest = KeyRequest::try_from_bytes(&sealed.keyrequest)?;
    let mut unsealing_key = keyrequest.derive_unsealing_key(label)?;

    let mut ciphertext = sealed.ciphertext.into_owned();
    let plaintext_ref =
        unsealing_key.open_in_place(Aad::empty(), &mut ciphertext)?;
    let plaintext_len = plaintext_ref.len();

    // unsealing happens in-place. set the length of the now decrypted
    // ciphertext blob and return that.
    ciphertext.truncate(plaintext_len);
    Ok(ciphertext)
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;

    use super::*;
    use crate::rng::SysRng;
    use crate::sha256;

    #[test]
    fn test_constants() {
        assert_eq!(AES_256_GCM.tag_len(), TAG_LEN);
        assert_eq!(sha256::digest(HKDF_SALT_STR).as_ref(), HKDF_SALT);
    }

    #[test]
    fn test_sealing_roundtrip_basic() {
        let mut rng = SysRng::new();

        let sealed = seal(&mut rng, b"", b"").unwrap();
        let unsealed = unseal(b"", sealed).unwrap();
        assert_eq!(&unsealed, b"");

        let sealed = seal(&mut rng, b"cool label", b"cool data").unwrap();
        let unsealed = unseal(b"cool label", sealed).unwrap();
        assert_eq!(&unsealed, b"cool data");
    }

    #[test]
    fn test_sealing_roundtrip_proptest() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();

        proptest!(|(label in arb_label, data in arb_data)| {
            let mut rng = SysRng::new();
            let sealed = seal(&mut rng, &label, &data).unwrap();
            let unsealed = unseal(&label, sealed).unwrap();
            assert_eq!(&data, &unsealed);
        });
    }
}
