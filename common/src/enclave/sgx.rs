//! SGX-specific implementations for in-enclave APIs

use std::borrow::Cow;

use ring::{
    aead::{
        Aad, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey,
        UnboundKey, AES_256_GCM,
    },
    hkdf::{self, HKDF_SHA256},
};
use secrecy::zeroize::Zeroizing;
use sgx_isa::{Keyname, Keypolicy};

use crate::{
    const_assert_usize_eq,
    enclave::{
        attributes, miscselect, xfrm, Error, MachineId, Measurement, Sealed,
        MIN_SGX_CPUSVN,
    },
    rng::Crng,
    sha256,
};

/// We salt the HKDF for domain separation purposes.
const HKDF_SALT: [u8; 32] =
    sha256::digest_const(b"LEXE-REALM::SgxSealing").into_inner();

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

// [`KeyRequest::TRUNCATED_SIZE`] is calculated manually and will need to be
// updated if [`sgx_isa::Keyrequest`] starts to include more fields.
//
// This block is a static assertion that [`sgx_isa::Keyrequest::_reserved2`]
// starts exactly [`KeyRequest::TRUNCATED_SIZE`] bytes into the struct. This
// static assertion should then cause a compile error if more fields are added
// (hopefully!).
const KEYREQUEST_RESERVED_BYTES_START: usize = {
    // Get a base pointer to the struct.
    let value = std::mem::MaybeUninit::<sgx_isa::Keyrequest>::uninit();
    let base_ptr: *const sgx_isa::Keyrequest = value.as_ptr();
    // `addr_of!` lets us create a pointer to the field without any intermediate
    // dereference like `&(*base_ptr)._reserved2 as *const _` would. This lets
    // us avoid any UB creating references to uninit data.
    let field_ptr = unsafe { std::ptr::addr_of!((*base_ptr)._reserved2) };

    // Compute the field offset.
    unsafe {
        (field_ptr as *const u8).offset_from(base_ptr as *const u8) as usize
    }
};
const_assert_usize_eq!(
    KeyRequest::TRUNCATED_SIZE,
    KEYREQUEST_RESERVED_BYTES_START
);

/// A convenience wrapper around an SGX [`sgx_isa::Keyrequest`].
struct KeyRequest(sgx_isa::Keyrequest);

impl KeyRequest {
    const UNPADDED_SIZE: usize = sgx_isa::Keyrequest::UNPADDED_SIZE; // 512

    /// The [`sgx_isa::Keyrequest`] struct size without the trailing reserved
    /// field.
    const TRUNCATED_SIZE: usize = 76; // == 512 - 436

    /// Generate a request for a unique, encrypt-at-most-once sealing key. The
    /// sealing key will only be recoverable on enclaves with the same
    /// `MRENCLAVE` measurement.
    fn gen_sealing_request(rng: &mut dyn Crng) -> Self {
        let mut keyid = [0u8; 32];
        rng.fill_bytes(&mut keyid);

        let attribute_mask: u64 = attributes::LEXE_MASK.bits();
        let xfrm_mask: u64 = xfrm::LEXE_MASK;
        let misc_mask: u32 = miscselect::LEXE_MASK.bits();

        // Since we only ever use the `MRENCLAVE` key policy, the ISVSVN doesn't
        // provide us any value. If there was a vulnerability discovered in the
        // node enclave, we would need to cut a new release, which has a
        // different MRENCLAVE anyway and therefore the old version can't unseal
        // the new data.
        let isvsvn = 0;

        // Commit to a _fixed_ CPUSVN checkpoint to reduce operational
        // complexity. When we need to bump the CPUSVN committed version (either
        // periodically or in response to a vulnerability disclosure), we'll cut
        // a new release (i.e., different MRENCLAVE) with the updated CPUSVN.
        // TODO(phlip9): update this just before prod release
        let cpusvn = MIN_SGX_CPUSVN;

        Self(sgx_isa::Keyrequest {
            keyname: Keyname::Seal as _,
            keypolicy: Keypolicy::MRENCLAVE,
            isvsvn,
            cpusvn: cpusvn.inner(),
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
            return Err(Error::InvalidKeyRequestLength);
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
    data: Cow<'_, [u8]>,
) -> Result<Sealed<'static>, Error> {
    let keyrequest = KeyRequest::gen_sealing_request(rng);
    let mut sealing_key = keyrequest.derive_sealing_key(label)?;

    let mut ciphertext = data.into_owned();
    let tag = sealing_key
        .seal_in_place_separate_tag(Aad::empty(), &mut ciphertext)
        .map_err(|_| Error::SealInputTooLarge)?;
    ciphertext.extend_from_slice(tag.as_ref());

    Ok(Sealed {
        keyrequest: keyrequest.as_bytes().to_vec().into(),
        ciphertext: Cow::Owned(ciphertext),
    })
}

pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    // the ciphertext is too small
    if sealed.ciphertext.len() < TAG_LEN {
        return Err(Error::UnsealInputTooSmall);
    }

    let keyrequest = KeyRequest::try_from_bytes(&sealed.keyrequest)?;
    let mut unsealing_key = keyrequest.derive_unsealing_key(label)?;

    let mut ciphertext = sealed.ciphertext.into_owned();
    let plaintext_ref = unsealing_key
        .open_in_place(Aad::empty(), &mut ciphertext)
        .map_err(|_| Error::UnsealDecryptionError)?;
    let plaintext_len = plaintext_ref.len();

    // unsealing happens in-place. set the length of the now decrypted
    // ciphertext blob and return that.
    ciphertext.truncate(plaintext_len);
    Ok(ciphertext)
}

pub fn measurement() -> Measurement {
    Measurement::new(sgx_isa::Report::for_self().mrenclave)
}

pub fn signer() -> Measurement {
    Measurement::new(sgx_isa::Report::for_self().mrsigner)
}

pub fn machine_id() -> MachineId {
    // use a fixed keyid
    let keyid = *b"~~~~ LEXE MACHINE ID KEY ID ~~~~";

    // bind the signer (lexe) not the enclave. this way all our enclaves can get
    // the same machine id
    let keypolicy = Keypolicy::MRSIGNER;

    // bind none, not even DEBUG
    let attribute_mask: u64 = 0;
    // bind none
    let xfrm_mask: u64 = 0;
    // bind none
    let misc_mask: u32 = 0;

    // Not a secret value. No reason to ever bump this.
    let isvsvn = 0;

    // Prevent very old platforms from getting the identifier. This is not a
    // security mitigation, just an early signal that a platform wasn't brought
    // up correctly.
    let cpusvn = MIN_SGX_CPUSVN;

    let keyrequest = sgx_isa::Keyrequest {
        keyname: Keyname::Seal as _,
        keypolicy,
        isvsvn,
        cpusvn: cpusvn.inner(),
        attributemask: [attribute_mask, xfrm_mask],
        miscmask: misc_mask,
        keyid,
        ..Default::default()
    };

    // This should never panic unless we run on very old SGX hardware.
    let bytes = keyrequest.egetkey().expect("Failed to get machine id");

    MachineId::new(bytes)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(AES_256_GCM.tag_len(), TAG_LEN);
    }
}
