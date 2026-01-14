//! SGX provides a number of ISA extensions which can be called in-enclave.
//! This module provides APIs for these along with related newtypes and consts.
//! Outside of SGX, dummy data is returned. See [`sgx_isa`] for more info.

use std::{borrow::Cow, fmt, io, mem, str::FromStr, sync::LazyLock};

use byte_array::ByteArray;
use bytes::{Buf, BufMut};
use cfg_if::cfg_if;
use lexe_std::array::{self, ArrayExt};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use ring::{
    aead::{
        AES_256_GCM, Aad, BoundKey, Nonce, NonceSequence, OpeningKey,
        SealingKey, UnboundKey,
    },
    hkdf::{self, HKDF_SHA256},
};
use secrecy::zeroize::Zeroizing;
use serde::{Deserialize, Serialize};
use sgx_isa::{Keyname, Keypolicy};
use thiserror::Error;

use crate::{
    enclave,
    env::DeployEnv,
    rng::{Crng, RngExt},
    serde_helpers::hexstr_or_bytes,
};

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
                    cpusvn: MinCpusvn::CURRENT.0,
                    miscselect: miscselect::LEXE_FLAGS,
                    _reserved1: [0; 28],
                    attributes: sgx_isa::Attributes {
                        // Just use prod value since the flags are fake anyway
                        flags: attributes::LEXE_FLAGS_PROD,
                        xfrm: xfrm::LEXE_FLAGS,
                    },
                    mrenclave: enclave::measurement().0,
                    _reserved2: [0; 32],
                    mrsigner: enclave::signer().0,
                    _reserved3: [0; 96],
                    isvprodid: 0u16,
                    isvsvn: 0u16,
                    _reserved4: [0; 60],
                    // This field is newtyped in `tls::attestation::quote`.
                    reportdata: [0; 64],
                    keyid: MachineId::KEY_ID,
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
            // Prefers `DEV_ENCLAVE`, otherwise defaults to `MOCK_ENCLAVE`.
            Measurement::DEV_ENCLAVE.unwrap_or(Measurement::MOCK_ENCLAVE)
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
            let keyid = MachineId::KEY_ID;

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
    rng: &mut dyn Crng,
    label: &[u8],
    data: Cow<'_, [u8]>,
) -> Result<Sealed<'static>, Error> {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            let keyrequest = LxKeyRequest::gen_sealing_request(rng);
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
            let keyrequest = MockKeyRequest::gen_sealing_request(rng);
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

#[derive(Debug, Error)]
pub enum Error {
    #[error("SGX error: {0:?}")]
    SgxError(sgx_isa::ErrorCode),

    #[error("sealing: input data is too large")]
    SealInputTooLarge,

    #[error("unsealing: ciphertext is too small")]
    UnsealInputTooSmall,

    #[error("keyrequest is not a valid length")]
    InvalidKeyRequestLength,

    #[error("unseal error: ciphertext or metadata may be corrupted")]
    UnsealDecryptionError,

    #[error("deserialize: input is malformed")]
    DeserializationError,
}

/// An enclave measurement.
///
/// Get the current enclave measurement with [`measurement`].
/// Get the current signer measurement with [`signer`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Measurement(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// A [`Measurement`] shortened to its first four bytes (8 hex chars).
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, Eq, PartialEq, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct MrShort(#[serde(with = "hexstr_or_bytes")] [u8; 4]);

/// A unique identifier for a particular hardware enclave.
///
/// The intention with this identifier is to compactly communicate whether a
/// piece of hardware with this id (in its current state) has the _capability_
/// to [`unseal`] some [`Sealed`] data that was [`seal`]ed on hardware
/// possessing the same id.
///
/// An easy way to show unsealing capability is to actually derive a key from
/// the SGX platform. Rather than encrypting some data with this key, we'll
/// instead use it as a public identifier. For different enclaves to still
/// derive the same machine id, we'll use the enclave signer (Lexe) in our key
/// derivation instead of the per-enclave measurement.
///
/// As an added bonus, if the machine operator ever bumps the [`OWNER_EPOCH`] in
/// the BIOS, the machine id will automatically change to a different value,
/// correctly reflecting this machine's inability to unseal the old data.
///
/// NOTE: on SGX this capability is modulo the [CPUSVN] (i.e., doesn't commit to
///       the CPUSVN) since it allows us to easily upgrade the SGX TCB platform
///       without needing to also online-migrate sealed enclave state.
///
/// [CPUSVN]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#security-version-number-svn
/// [`OWNER_EPOCH`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#owner-epoch
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct MachineId(#[serde(with = "hexstr_or_bytes")] [u8; 16]);

/// TODO(max): Needs docs
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, Eq, PartialEq, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct MinCpusvn(#[serde(with = "hexstr_or_bytes")] [u8; 16]);

/// Sealed and encrypted data
// TODO(phlip9): use a real serialization format like CBOR or something
// TODO(phlip9): additional authenticated data?
#[derive(Eq, PartialEq)]
pub struct Sealed<'a> {
    /// A truncated [`sgx_isa::Keyrequest`].
    ///
    /// This field contains all the data needed to correctly recover the
    /// underlying seal key material inside an enclave. Currently this field is
    /// 76 bytes in SGX.
    keyrequest: Cow<'a, [u8]>,
    /// Encrypted ciphertext
    ciphertext: Cow<'a, [u8]>,
}

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

// --- impl Error --- //

impl From<sgx_isa::ErrorCode> for Error {
    fn from(err: sgx_isa::ErrorCode) -> Self {
        Self::SgxError(err)
    }
}

// --- impl Measurement --- //

impl Measurement {
    pub const MOCK_ENCLAVE: Self =
        Self::new(*b"~~~~~~~ LEXE MOCK ENCLAVE ~~~~~~");
    pub const MOCK_SIGNER: Self =
        Self::new(*b"======= LEXE MOCK SIGNER =======");

    /// A dev enclave measurement, only applicable to non-SGX dev builds, which
    /// allows nearly-identical local binaries to have a differing measurements
    /// accessible at run time, without the need to wire through CLI args.
    #[cfg_attr(target_env = "sgx", allow(dead_code))]
    const DEV_ENCLAVE: Option<Self> = match option_env!("DEV_MEASUREMENT") {
        // Panics at compile time if DEV_MEASUREMENT isn't valid [u8; 32] hex
        Some(hex) => Some(Self::new(hex::decode_const(hex.as_bytes()))),
        // Option::map is not const
        None => None,
    };

    /// The enclave signer measurement our debug enclaves are signed with.
    /// This is also the measurement of the fortanix/rust-sgx dummy key:
    /// <https://github.com/fortanix/rust-sgx/blob/master/intel-sgx/enclave-runner/src/dummy.key>
    ///
    /// Running an enclave with `run-sgx .. --debug` will automatically sign
    /// with this key just before running.
    pub const DEV_SIGNER: Self = Self::new(hex::decode_const(
        b"9affcfae47b848ec2caf1c49b4b283531e1cc425f93582b36806e52a43d78d1a",
    ));

    /// The enclave signer measurement our production enclaves should be signed
    /// with. Inside an enclave, get the signer with [`signer`].
    pub const PROD_SIGNER: Self = Self::new(hex::decode_const(
        b"02d07f56b7f4a71d32211d6821beaeb316fbf577d02bab0dfe1f18a73de08a8e",
    ));

    /// Return the expected signer measurement by [`DeployEnv`] and whether
    /// we're in mock or sgx mode.
    pub const fn expected_signer(use_sgx: bool, env: DeployEnv) -> Self {
        if use_sgx {
            match env {
                DeployEnv::Prod | DeployEnv::Staging => Self::PROD_SIGNER,
                DeployEnv::Dev => Self::DEV_SIGNER,
            }
        } else {
            Self::MOCK_SIGNER
        }
    }

    /// Compute an enclave measurement from an `.sgxs` file stream
    /// [`std::io::Read`].
    ///
    /// * Enclave binaries are first converted to `.sgxs` files, which exactly
    ///   mirror the memory layout of the loaded enclave binaries right before
    ///   running.
    /// * Conveniently, the SHA-256 hash of an enclave `.sgxs` binary is exactly
    ///   the same as the actual enclave measurement hash, since the memory
    ///   layout is identical (caveat: unless we use some more sophisticated
    ///   extendable enclave features).
    pub fn compute_from_sgxs(
        mut sgxs_reader: impl io::Read,
    ) -> io::Result<Measurement> {
        let mut buf = [0u8; 4096];
        let mut digest = sha256::Context::new();

        loop {
            let n = sgxs_reader.read(&mut buf)?;
            if n == 0 {
                let hash = digest.finish();
                return Ok(Measurement::new(hash.to_array()));
            } else {
                digest.update(&buf[0..n]);
            }
        }
    }

    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn short(&self) -> MrShort {
        MrShort::from(self)
    }
}

byte_array::impl_byte_array!(Measurement, 32);
byte_array::impl_fromstr_fromhex!(Measurement, 32);
byte_array::impl_debug_display_as_hex!(Measurement);

// --- impl MrShort --- //

impl MrShort {
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Whether this [`MrShort`] is a prefix of the given [`Measurement`].
    pub fn is_prefix_of(&self, long: &Measurement) -> bool {
        self.0 == long.0[..4]
    }
}

byte_array::impl_byte_array!(MrShort, 4);
byte_array::impl_fromstr_fromhex!(MrShort, 4);
byte_array::impl_debug_display_as_hex!(MrShort);

impl From<&Measurement> for MrShort {
    fn from(long: &Measurement) -> Self {
        (long.0)[..4].try_into().map(Self).unwrap()
    }
}

// --- impl MachineId --- //

impl MachineId {
    // TODO(phlip9): use the machine id of my dev machine until we build a
    // proper get-machine-id bin util.
    pub const MOCK: Self =
        MachineId::new(hex::decode_const(b"52bc575eb9618084083ca7b3a45a2a76"));

    /// We use a fixed keyid
    const KEY_ID: [u8; 32] = *b"~~~~ LEXE MACHINE ID KEY ID ~~~~";

    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

byte_array::impl_byte_array!(MachineId, 16);
byte_array::impl_fromstr_fromhex!(MachineId, 16);
byte_array::impl_debug_display_as_hex!(MachineId);

// --- impl MinCpusvn --- //

impl MinCpusvn {
    /// This is the current CPUSVN we commit to when sealing data in SGX.
    ///
    /// Updated: 2024/12/04 - Linux SGX platform v2.25
    pub const CURRENT: Self =
        Self::new(hex::decode_const(b"0e0e100fffff01000000000000000000"));

    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn inner(self) -> [u8; 16] {
        self.0
    }
}

impl ByteArray<16> for MinCpusvn {
    fn from_array(array: [u8; 16]) -> Self {
        Self(array)
    }
    fn to_array(&self) -> [u8; 16] {
        self.0
    }
    fn as_array(&self) -> &[u8; 16] {
        &self.0
    }
}

impl FromStr for MinCpusvn {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from_hexstr(s)
    }
}

impl fmt::Display for MinCpusvn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Self::fmt_hexstr(self, f)
    }
}

impl fmt::Debug for MinCpusvn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MinCpusvn")
            .field(&self.hex_display())
            .finish()
    }
}

// --- impl Sealed --- //

impl<'a> Sealed<'a> {
    /// AES-256-GCM tag length
    pub const TAG_LEN: usize = 16;

    /// We salt the HKDF for domain separation purposes.
    const HKDF_SALT: [u8; 32] = array::pad(*b"LEXE-REALM::SgxSealing");

    pub fn serialize(&self) -> Vec<u8> {
        let out_len = mem::size_of::<u32>()
            + self.keyrequest.len()
            + mem::size_of::<u32>()
            + self.ciphertext.len();
        let mut out = Vec::with_capacity(out_len);

        out.put_u32_le(self.keyrequest.len() as u32);
        out.put(self.keyrequest.as_ref());
        out.put_u32_le(self.ciphertext.len() as u32);
        out.put(self.ciphertext.as_ref());
        out
    }

    pub fn deserialize(bytes: &'a [u8]) -> Result<Self, Error> {
        let (keyrequest, bytes) = Self::read_bytes(bytes)?;
        let (ciphertext, bytes) = Self::read_bytes(bytes)?;

        if bytes.is_empty() {
            Ok(Self {
                keyrequest: Cow::Borrowed(keyrequest),
                ciphertext: Cow::Borrowed(ciphertext),
            })
        } else {
            Err(Error::DeserializationError)
        }
    }

    // Helper to split a byte slice into a 4 byte little-endian slice and the
    // remainder. Errors if the input slice is smaller than 4 bytes.
    fn read_bytes(bytes: &[u8]) -> Result<(&[u8], &[u8]), Error> {
        let (len, bytes) = Self::read_u32_le(bytes)?;
        let len = len as usize;
        if bytes.len() >= len {
            Ok(bytes.split_at(len))
        } else {
            Err(Error::DeserializationError)
        }
    }

    // Reads a little-endian u32 from the start of a slice. Returns the u32 and
    // the remainder, or errors if there aren't enough bytes.
    fn read_u32_le(mut bytes: &[u8]) -> Result<(u32, &[u8]), Error> {
        if bytes.len() >= mem::size_of::<u32>() {
            Ok((bytes.get_u32_le(), bytes))
        } else {
            Err(Error::DeserializationError)
        }
    }
}

impl fmt::Debug for Sealed<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sealed")
            .field("keyrequest", &hex::display(&self.keyrequest))
            .field("ciphertext", &hex::display(&self.ciphertext))
            .finish()
    }
}

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
    fn gen_sealing_request(mut rng: &mut dyn Crng) -> Self {
        let keyid = rng.gen_bytes();

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
            keyid,
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

    fn derive_key_material(&self) -> Result<Zeroizing<[u8; 16]>, Error> {
        cfg_if! {
            if #[cfg(target_env = "sgx")] {
                Ok(Zeroizing::new(self.0.egetkey()?))
            } else {
                unimplemented!()
            }
        }
    }

    fn derive_aesgcm_key(key_material: &[u8; 16], label: &[u8]) -> UnboundKey {
        UnboundKey::from(
            hkdf::Salt::new(HKDF_SHA256, &Sealed::HKDF_SALT)
                .extract(key_material.as_slice())
                .expand(&[label], &AES_256_GCM)
                .expect("Failed to derive sealing key from key material"),
        )
    }
}

#[cfg(not(target_env = "sgx"))] // cfg under not-SGX for safety.
impl MockKeyRequest {
    fn gen_sealing_request(mut rng: &mut dyn Crng) -> Self {
        let keyid = rng.gen_bytes();
        Self { keyid }
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

// --- SGX feature flag consts --- //

// SGX platform feature flags vs masks
//
// Each feature set has two components: a MASK, which determines _which_
// features we want to bind, and then the FLAGS, which determines the _value_
// (on/off) of each (bound) feature. All bits in FLAGS that aren't also set in
// MASK are ignored.
//
// As a simplified example, let's look at a system with only two features:
// DEBUG = 0b01 and FAST = 0b10. Suppose DEBUG enables the enclave DEBUG mode,
// which isn't safe for production, while FAST enables some high-performance CPU
// hardware feature.
//
// If we're building an enclave for production, we care about the values of both
// features, so we set MASK = 0b11. It would be a problem if our enclave ran
// with DEBUG enabled and FAST turned off, so we set FLAGS = 0b10.
//
// In the event we don't care about the FAST feature (maybe we need to run on
// both old and new CPUs), we would instead only bind the DEBUG feature, with
// MASK = 0b01. In this case, only the value of the DEBUG bit matters and the
// FAST bit is completely ignored, so FLAGS = 0b01 and 0b11 are both equivalent.
//
// By not including a feature in our MASK, we are effectively allowing
// the platform to determine the value at runtime.

/// SGX platform feature flags
///
/// See: [`AttributesFlags`](sgx_isa::AttributesFlags)
pub mod attributes {
    use sgx_isa::AttributesFlags;

    /// In production, ensure the SGX debug flag is turned off. We're also not
    /// currently using any other features, like KSS (Key sharing and
    /// separation) or AEX Notify (Async Enclave eXit Notify).
    pub const LEXE_FLAGS_PROD: AttributesFlags = AttributesFlags::MODE64BIT;

    pub const LEXE_FLAGS_DEBUG: AttributesFlags =
        LEXE_FLAGS_PROD.union(AttributesFlags::DEBUG);

    /// The flags we want to bind (whether on or off).
    pub const LEXE_MASK: AttributesFlags = AttributesFlags::INIT
        .union(AttributesFlags::DEBUG)
        .union(AttributesFlags::MODE64BIT);
}

/// XFRM: CPU feature flags
///
/// See: <https://github.com/intel/linux-sgx> `common/inc/sgx_attributes.h`
pub mod xfrm {
    /// Legacy XFRM which includes the basic feature bits required by SGX
    /// x87 state(0x01) and SSE state(0x02).
    pub const LEGACY: u64 = 0x0000000000000003;
    /// AVX XFRM which includes AVX state(0x04) and SSE state(0x02) required by
    /// AVX.
    pub const AVX: u64 = 0x0000000000000006;
    /// AVX-512 XFRM.
    pub const AVX512: u64 = 0x00000000000000e6;
    /// MPX XFRM - not supported.
    pub const MPX: u64 = 0x0000000000000018;
    /// PKRU state.
    pub const PKRU: u64 = 0x0000000000000200;
    /// AMX XFRM, including XTILEDATA(0x40000) and XTILECFG(0x20000).
    pub const AMX: u64 = 0x0000000000060000;

    /// Features required by LEXE enclaves.
    ///
    /// Absolutely mandatory requirements:
    ///   + SSE3+ (basic vectorization)
    ///   + AESNI (AES crypto accelerators).
    ///
    /// Full AVX512 is not required but at least ensures we're running on more
    /// recent hardware.
    pub const LEXE_FLAGS: u64 = AVX512 | LEGACY;

    /// Require LEXE features but allow new ones, determined at runtime.
    pub const LEXE_MASK: u64 = LEXE_FLAGS;
}

/// SGX platform MISCSELECT feature flags
pub mod miscselect {
    use sgx_isa::Miscselect;

    /// Don't need any features.
    pub const LEXE_FLAGS: Miscselect = Miscselect::empty();

    /// Bind nothing.
    pub const LEXE_MASK: Miscselect = Miscselect::empty();
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assume, proptest, strategy::Strategy};
    use ring::aead::AES_256_GCM;

    use super::*;
    use crate::{rng::FastRng, test_utils::roundtrip};

    // TODO(phlip9): test KeyRequest mutations
    // TODO(phlip9): test truncate/extend mutations

    #[test]
    fn serde_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<Measurement>();
        roundtrip::json_string_roundtrip_proptest::<MachineId>();
        roundtrip::json_string_roundtrip_proptest::<MinCpusvn>();
    }

    #[test]
    fn fromstr_display_roundtrips() {
        roundtrip::fromstr_display_roundtrip_proptest::<Measurement>();
        roundtrip::fromstr_display_roundtrip_proptest::<MachineId>();
        roundtrip::fromstr_display_roundtrip_proptest::<MinCpusvn>();
    }

    #[test]
    fn test_measurement_consistent() {
        let m1 = super::measurement();
        let m2 = super::measurement();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_machine_id_consistent() {
        let m1 = super::machine_id();
        let m2 = super::machine_id();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_mr_short() {
        proptest!(|(
            long1 in any::<Measurement>(),
            long2 in any::<Measurement>(),
        )| {
            let short1 = long1.short();
            let short2 = long2.short();
            assert!(short1.is_prefix_of(&long1));
            assert!(short2.is_prefix_of(&long2));

            if short1 != short2 {
                assert_ne!(long1, long2);
                assert!(!short1.is_prefix_of(&long2));
                assert!(!short2.is_prefix_of(&long1));
            }
        });
    }

    #[test]
    fn test_sealed_serialization() {
        let arb_keyrequest = any::<Vec<u8>>();
        let arb_ciphertext = any::<Vec<u8>>();
        let arb_sealed = (arb_keyrequest, arb_ciphertext).prop_map(
            |(keyrequest, ciphertext)| Sealed {
                keyrequest: keyrequest.into(),
                ciphertext: ciphertext.into(),
            },
        );

        proptest!(|(sealed in arb_sealed)| {
            let bytes = sealed.serialize();
            let sealed2 = Sealed::deserialize(&bytes).unwrap();
            assert_eq!(sealed, sealed2);
        });
    }

    #[test]
    fn test_sealing_roundtrip_basic() {
        let mut rng = FastRng::new();

        let sealed = super::seal(&mut rng, b"", b"".as_slice().into()).unwrap();
        let unsealed = super::unseal(sealed, b"").unwrap();
        assert_eq!(&unsealed, b"");

        let sealed = super::seal(
            &mut rng,
            b"cool label",
            b"cool data".as_slice().into(),
        )
        .unwrap();
        let unsealed = super::unseal(sealed, b"cool label").unwrap();
        assert_eq!(&unsealed, b"cool data");
    }

    #[test]
    fn test_sealing_roundtrip_proptest() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();

        proptest!(|(mut rng in any::<FastRng>(), label in arb_label, data in arb_data)| {
            let sealed = super::seal(&mut rng, &label, data.clone().into()).unwrap();
            let unsealed = super::unseal(sealed, &label).unwrap();
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
            mut rng in any::<FastRng>(),
            label in arb_label,
            data in arb_data,
            mutation in arb_mutation,
        )| {
            let sealed = super::seal(&mut rng, &label, data.into()).unwrap();

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
            super::unseal(sealed, &label).unwrap_err();
        });
    }

    #[test]
    fn test_constants() {
        assert_eq!(AES_256_GCM.tag_len(), Sealed::TAG_LEN);
    }
}
