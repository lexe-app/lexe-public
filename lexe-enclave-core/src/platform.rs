//! SGX provides a number of ISA extensions which can be called in-enclave.
//! This module provides APIs for these. Outside of SGX, dummy data is
//! returned. See [`sgx_isa`] for more info.

use std::{borrow::Cow, sync::LazyLock};

use cfg_if::cfg_if;
#[cfg(not(target_env = "sgx"))]
use lexe_byte_array::ByteArray;
#[cfg(not(target_env = "sgx"))]
use lexe_hex::hex;
use lexe_std::array::{self, ArrayExt};
use ring::{
    aead::{
        AES_256_GCM, Aad, BoundKey, Nonce, NonceSequence, OpeningKey,
        SealingKey, UnboundKey,
    },
    hkdf::{self, HKDF_SHA256},
};
use sgx_isa::{Keyname, Keypolicy};

use crate::{
    platform as enclave,
    types::{
        Error, MachineId, Measurement, MinCpusvn, Sealed, attributes,
        miscselect, xfrm,
    },
};

// TODO(phlip9): if `ring` ever supports zeroizing, take a pass at zeroizing
// all secrets in here.

// --- SGX 'Platform APIs' --- //
// These call ISA extensions provided by SGX, or return dummy data if non-SGX.

/// Get an [`sgx_isa::Report`] for the current enclave by calling [`EREPORT`].
///
/// [`EREPORT`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#report-ereport
pub fn report() -> &'static sgx_isa::Report {
    static SELF_REPORT: LazyLock<sgx_isa::Report> = LazyLock::new(|| {
        cfg_if! {
            if #[cfg(target_env = "sgx")] {
                sgx_isa::Report::for_self()
            } else {
                sgx_isa::Report {
                    cpusvn: MinCpusvn::CURRENT.to_array(),
                    miscselect: miscselect::LEXE_FLAGS,
                    _reserved1: [0; 28],
                    attributes: sgx_isa::Attributes {
                        // Just use prod value since the flags are fake anyway
                        flags: attributes::LEXE_FLAGS_PROD,
                        xfrm: xfrm::LEXE_FLAGS,
                    },
                    mrenclave: enclave::measurement().to_array(),
                    _reserved2: [0; 32],
                    mrsigner: enclave::signer().to_array(),
                    _reserved3: [0; 96],
                    isvprodid: 0u16,
                    isvsvn: 0u16,
                    _reserved4: [0; 60],
                    // This is newtyped in `lexe_tls_attest_server::quote`
                    reportdata: [0; 64],
                    keyid: MACHINE_ID_KEY_ID,
                    mac: [0; 16],
                }
            }
        }
    });

    &SELF_REPORT
}

/// Return the current enclave measurement.
///
/// + In SGX, this is often called the [`MRENCLAVE`].
/// + In mock mode, this returns a fixed value.
/// + The enclave measurement is a SHA-256 hash summary of the enclave code and
///   initial memory contents.
/// + This hash uniquely identifies an enclave; any change to the code will also
///   change the measurement.
///
/// [`MRENCLAVE`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#enclave-measurement-mrenclave
pub fn measurement() -> Measurement {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            Measurement::new(enclave::report().mrenclave)
        } else {
            // Prefers `$DEV_MEASUREMENT`, otherwise defaults to `MOCK_ENCLAVE`
            match option_env!("DEV_MEASUREMENT") {
                // Panics at compile time if DEV_MEASUREMENT isn't valid
                // [u8; 32] hex
                Some(hex) => Measurement::new(hex::decode_const(hex.as_bytes())),
                // Option::map is not const
                None => Measurement::MOCK_ENCLAVE,
            }
        }
    }
}

/// Retrieve the enclave signer measurement of the current running enclave.
/// Every enclave is signed with an RSA keypair, plus some extra metadata.
///
/// + In SGX, this value is called the [`MRSIGNER`].
/// + In mock mode, this returns a fixed value.
/// + The signer is a 3072-bit RSA keypair with exponent=3. The signer
///   measurement is the SHA-256 hash of the (little-endian) public key modulus.
///
/// [`MRSIGNER`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#signer-measurement-mrsigner
pub fn signer() -> Measurement {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            Measurement::new(enclave::report().mrsigner)
        } else {
            Measurement::MOCK_SIGNER
        }
    }
}

/// Get the current machine id from inside an enclave.
pub fn machine_id() -> MachineId {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            use sgx_isa::{Keyname, Keypolicy};

            // use a fixed keyid
            let keyid = MACHINE_ID_KEY_ID;

            // bind the signer (lexe) not the enclave. this way all our
            // enclaves can get the same machine id
            let keypolicy = Keypolicy::MRSIGNER;

            // bind none, not even DEBUG
            let attribute_mask: u64 = 0;
            // bind none
            let xfrm_mask: u64 = 0;
            // bind none
            let misc_mask: u32 = 0;

            // Not a secret value. No reason to ever bump this.
            let isvsvn = 0;

            // Prevent very old platforms from getting the identifier. This
            // is not a security mitigation, just an early signal that a
            // platform wasn't brought up correctly.
            let cpusvn = MinCpusvn::CURRENT;

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
            let bytes =
                keyrequest.egetkey().expect("Failed to get machine id");

            MachineId::new(bytes)
        } else {
            MachineId::MOCK
        }
    }
}

/// Seal and encrypt data in an enclave so that it's only readable inside
/// another enclave running the same software.
///
/// Users should also provide a unique domain-separation `label` for each
/// unique sealing type or location.
///
/// Data is currently encrypted with AES-256-GCM using the [`ring`] backend.
///
/// In SGX, this sealed data is only readable by other enclave instances
/// with the exact same [`MRENCLAVE`] measurement. The sealing key also
/// commits to the platform [CPUSVN], meaning enclaves running on platforms
/// with out-of-date SGX TCB will be unable to unseal data sealed on updated
/// SGX platforms.
///
/// SGX sealing keys are sampled uniquely and only used to encrypt data
/// once. In effect, the `keyid` is a nonce but the key itself is only
/// deriveable inside an enclave with an exactly matching [`MRENCLAVE`]
/// (among other things).
///
/// [`MRENCLAVE`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#enclave-measurement-mrenclave
/// [CPUSVN]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#security-version-number-svn
pub fn seal(
    random_keyid: [u8; 32],
    label: &[u8],
    data: Cow<'_, [u8]>,
) -> Result<Sealed<'static>, Error> {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            let keyrequest = LxKeyRequest::new_sealing_request(random_keyid);
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
        } else {
            let keyrequest = MockKeyRequest::new_sealing_request(random_keyid);
            let key = keyrequest.derive_key(label);
            let mut ciphertext = data.into_owned();
            let nonce = Nonce::assume_unique_for_key([0u8; 12]);
            key
                .seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
                .map_err(|_| Error::SealInputTooLarge)?;
            Ok(Sealed {
                keyrequest: keyrequest.as_bytes().to_vec().into(),
                ciphertext: Cow::Owned(ciphertext),
            })
        }
    }
}

/// Unseal and decrypt data previously sealed with [`seal`].
pub fn unseal(sealed: Sealed<'_>, label: &[u8]) -> Result<Vec<u8>, Error> {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            // the ciphertext is too small
            if sealed.ciphertext.len() < Sealed::TAG_LEN {
                return Err(Error::UnsealInputTooSmall);
            }

            let keyrequest =
                LxKeyRequest::try_from_bytes(&sealed.keyrequest)?;
            let mut unsealing_key = keyrequest.derive_unsealing_key(label)?;

            let mut ciphertext = sealed.ciphertext.into_owned();
            let plaintext_ref = unsealing_key
                .open_in_place(Aad::empty(), &mut ciphertext)
                .map_err(|_| Error::UnsealDecryptionError)?;
            let plaintext_len = plaintext_ref.len();

            // unsealing happens in-place. set the length of the now
            // decrypted ciphertext blob and return that.
            ciphertext.truncate(plaintext_len);
            Ok(ciphertext)
        } else {
            let keyrequest =
                MockKeyRequest::try_from_bytes(&sealed.keyrequest)?;
            let key = keyrequest.derive_key(label);
            let nonce = Nonce::assume_unique_for_key([0u8; 12]);

            let mut ciphertext = sealed.ciphertext.into_owned();
            let plaintext_ref = key
                .open_in_place(nonce, Aad::empty(), &mut ciphertext)
                .map_err(|_| Error::UnsealDecryptionError)?;
            let plaintext_len = plaintext_ref.len();

            // unsealing happens in-place. set the length of the now
            // decrypted ciphertext blob and return that.
            ciphertext.truncate(plaintext_len);
            Ok(ciphertext)
        }
    }
}

// --- Types --- //

/// A convenience wrapper around an SGX [`sgx_isa::Keyrequest`].
#[cfg_attr(not(target_env = "sgx"), allow(dead_code))]
struct LxKeyRequest(sgx_isa::Keyrequest);

/// Key request for a mock sealing implementation. It just samples a fresh key
/// for every sealing operation and stores the key adjacent to the ciphertext.
///
/// NOTE: this does not provide any security whatsoever.
#[cfg(not(target_env = "sgx"))] // cfg under not-SGX for safety.
struct MockKeyRequest {
    keyid: [u8; 32],
}

/// A nonce wrapper that panics if a `Nonce` is seal/unseal'ed more than once.
struct OnlyOnce(Option<Nonce>);

// --- impl MachineId --- //

/// We use a fixed keyid
const MACHINE_ID_KEY_ID: [u8; 32] = *b"~~~~ LEXE MACHINE ID KEY ID ~~~~";

// --- impl LxKeyRequest and MockKeyRequest --- //

lexe_std::const_assert_usize_eq!(
    LxKeyRequest::TRUNCATED_SIZE,
    LxKeyRequest::KEYREQUEST_RESERVED_BYTES_START
);

#[cfg_attr(not(target_env = "sgx"), allow(dead_code))]
impl LxKeyRequest {
    const UNPADDED_SIZE: usize = sgx_isa::Keyrequest::UNPADDED_SIZE; // 512

    /// The [`sgx_isa::Keyrequest`] struct size without the trailing reserved
    /// field.
    const TRUNCATED_SIZE: usize = 76; // == 512 - 436

    // [`LxKeyRequest::TRUNCATED_SIZE`] is calculated manually and will need to
    // be updated if [`sgx_isa::Keyrequest`] starts to include more fields.
    //
    // This block is a static assertion that [`sgx_isa::Keyrequest::_reserved2`]
    // starts exactly [`LxKeyRequest::TRUNCATED_SIZE`] bytes into the struct.
    // This static assertion should then cause a compile error if more fields
    // are added (hopefully!).
    #[allow(dead_code)] // This is just an assertion
    const KEYREQUEST_RESERVED_BYTES_START: usize = {
        // Get a base pointer to the struct.
        let value = std::mem::MaybeUninit::<sgx_isa::Keyrequest>::uninit();
        let base_ptr: *const sgx_isa::Keyrequest = value.as_ptr();
        // `addr_of!` lets us create a pointer to the field without any
        // intermediate dereference like `&(*base_ptr)._reserved2 as *const _`
        // would. This lets us avoid any UB creating references to uninit data.
        let field_ptr = unsafe { std::ptr::addr_of!((*base_ptr)._reserved2) };

        // Compute the field offset.
        unsafe {
            (field_ptr as *const u8).offset_from(base_ptr as *const u8) as usize
        }
    };

    /// Generate a request for a unique, encrypt-at-most-once sealing key. The
    /// sealing key will only be recoverable on enclaves with the same
    /// `MRENCLAVE` measurement.
    fn new_sealing_request(random_keyid: [u8; 32]) -> Self {
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
        let cpusvn = MinCpusvn::CURRENT;

        Self(sgx_isa::Keyrequest {
            keyname: Keyname::Seal as _,
            keypolicy: Keypolicy::MRENCLAVE,
            isvsvn,
            cpusvn: cpusvn.inner(),
            attributemask: [attribute_mask, xfrm_mask],
            miscmask: misc_mask,
            keyid: random_keyid,
            ..Default::default()
        })
    }

    /// Truncate a full 512 SGX [`LxKeyRequest`] to 76 bytes, leaving off the
    /// empty reserved bytes. This makes our `Sealed` data significantly
    /// more compact.
    fn as_bytes(&self) -> &[u8] {
        let bytes: &[u8] = self.0.as_ref();
        &bytes[0..Self::TRUNCATED_SIZE]
    }

    /// Deserialize a [`LxKeyRequest`] from bytes. Inputs must be between 76 B
    /// and 512 B.
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

    /// We sample a unique sealing key per seal request. just grab the random
    /// part of the label as a nonce.
    fn single_use_nonce(&self) -> OnlyOnce {
        let (_, nonce) = self.0.keyid.rsplit_array_ref_stable::<12>();
        OnlyOnce::new(Nonce::assume_unique_for_key(*nonce))
    }

    // TODO(phlip9): will need wrapper types around SealingKey and OpeningKey to
    // successfully implement Zeroize...

    fn derive_sealing_key(
        &self,
        label: &[u8],
    ) -> Result<SealingKey<OnlyOnce>, Error> {
        let key_material = self.derive_key_material()?;
        Ok(SealingKey::new(
            Self::derive_aesgcm_key(&key_material, label),
            self.single_use_nonce(),
        ))
    }

    fn derive_unsealing_key(
        &self,
        label: &[u8],
    ) -> Result<OpeningKey<OnlyOnce>, Error> {
        let key_material = self.derive_key_material()?;
        Ok(OpeningKey::new(
            Self::derive_aesgcm_key(&key_material, label),
            self.single_use_nonce(),
        ))
    }

    fn derive_key_material(&self) -> Result<[u8; 16], Error> {
        cfg_if! {
            if #[cfg(target_env = "sgx")] {
                Ok(self.0.egetkey()?)
            } else {
                unimplemented!()
            }
        }
    }

    fn derive_aesgcm_key(key_material: &[u8; 16], label: &[u8]) -> UnboundKey {
        /// We salt the HKDF for domain separation purposes.
        const HKDF_SALT: [u8; 32] = array::pad(*b"LEXE-REALM::SgxSealing");
        UnboundKey::from(
            hkdf::Salt::new(HKDF_SHA256, &HKDF_SALT)
                .extract(key_material.as_slice())
                .expand(&[label], &AES_256_GCM)
                .expect("Failed to derive sealing key from key material"),
        )
    }
}

#[cfg(not(target_env = "sgx"))] // cfg under not-SGX for safety.
impl MockKeyRequest {
    fn new_sealing_request(random_keyid: [u8; 32]) -> Self {
        Self {
            keyid: random_keyid,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        self.keyid.as_slice()
    }

    fn try_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let keyid = <[u8; 32]>::try_from(bytes)
            .map_err(|_| Error::InvalidKeyRequestLength)?;
        Ok(Self { keyid })
    }

    fn derive_key(&self, label: &[u8]) -> ring::aead::LessSafeKey {
        ring::aead::LessSafeKey::new(UnboundKey::from(
            hkdf::Salt::new(HKDF_SHA256, &[0x42; 32])
                .extract(self.keyid.as_slice())
                .expand(&[label], &AES_256_GCM)
                .expect("Failed to derive sealing key from key material"),
        ))
    }
}

// --- impl OnlyOnce --- //

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

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assume, proptest, strategy::Strategy};
    use ring::aead::AES_256_GCM;

    use super::*;

    // TODO(phlip9): test KeyRequest mutations
    // TODO(phlip9): test truncate/extend mutations

    #[test]
    fn test_measurement_consistent() {
        let m1 = enclave::measurement();
        let m2 = enclave::measurement();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_machine_id_consistent() {
        let m1 = enclave::machine_id();
        let m2 = enclave::machine_id();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_sealing_roundtrip_basic() {
        let random_keyid = [0x69; 32];
        let sealed =
            enclave::seal(random_keyid, b"", b"".as_slice().into()).unwrap();
        let unsealed = enclave::unseal(sealed, b"").unwrap();
        assert_eq!(&unsealed, b"");

        let sealed = enclave::seal(
            random_keyid,
            b"cool label",
            b"cool data".as_slice().into(),
        )
        .unwrap();
        let unsealed = enclave::unseal(sealed, b"cool label").unwrap();
        assert_eq!(&unsealed, b"cool data");
    }

    #[test]
    fn test_sealing_roundtrip_proptest() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();

        proptest!(|(
            random_keyid in any::<[u8; 32]>(),
            label in arb_label,
            data in arb_data,
        )| {
            let sealed = enclave::seal(random_keyid, &label, data.clone().into()).unwrap();
            let unsealed = enclave::unseal(sealed, &label).unwrap();
            assert_eq!(&data, &unsealed);
        });
    }

    #[test]
    fn test_sealing_detects_ciphertext_change() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();
        let arb_mutation = any::<Vec<u8>>()
            .prop_filter("can't be empty or all zeroes", |m| {
                !m.is_empty() && !m.iter().all(|x| x == &0u8)
            });

        proptest!(|(
            random_keyid in any::<[u8; 32]>(),
            label in arb_label,
            data in arb_data,
            mutation in arb_mutation,
        )| {
            let sealed = enclave::seal(random_keyid, &label, data.into()).unwrap();

            let keyrequest = sealed.keyrequest;
            let ciphertext_original = sealed.ciphertext.into_owned();
            let mut ciphertext = ciphertext_original.clone();

            for (c, m) in ciphertext.iter_mut().zip(mutation.iter()) {
                *c ^= m;
            }

            prop_assume!(ciphertext != ciphertext_original);

            let sealed = Sealed {
                keyrequest,
                ciphertext: ciphertext.into(),
            };

            // TODO(phlip9): check error
            enclave::unseal(sealed, &label).unwrap_err();
        });
    }

    #[test]
    fn test_constants() {
        assert_eq!(AES_256_GCM.tag_len(), Sealed::TAG_LEN);
    }
}
