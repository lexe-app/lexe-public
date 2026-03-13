use std::{borrow::Cow, error::Error as StdError, fmt, io, mem, str::FromStr};

use lexe_byte_array::ByteArray;
use lexe_hex::hex;
use lexe_serde::hexstr_or_bytes;
use lexe_sha256::sha256;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};

/// An SGX enclave measurement (MRENCLAVE): a SHA-256 hash of the enclave
/// binary, used to verify node integrity. Serialized as a 64-character hex
/// string.
//
// Get the current enclave measurement with [`measurement`].
// Get the current signer measurement with [`signer`].
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

pub enum Error {
    SgxError(sgx_isa::ErrorCode),
    SealInputTooLarge,
    UnsealInputTooSmall,
    InvalidKeyRequestLength,
    UnsealDecryptionError,
    DeserializationError,
}

/// A unique identifier for a particular hardware enclave.
///
/// The intention with this identifier is to compactly communicate whether a
/// piece of hardware with this id (in its current state) has the _capability_
/// to `unseal` some [`Sealed`] data that was `seal`ed on hardware
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
    pub keyrequest: Cow<'a, [u8]>,
    /// Encrypted ciphertext
    pub ciphertext: Cow<'a, [u8]>,
}

// --- impl Error --- //

impl StdError for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::SgxError(_) => "",
            Self::SealInputTooLarge => "sealing: input data is too large",
            Self::UnsealInputTooSmall => "unsealing: ciphertext is too small",
            Self::InvalidKeyRequestLength => "keyrequest is not a valid length",
            Self::UnsealDecryptionError =>
                "unseal error: ciphertext or metadata may be corrupted",
            Self::DeserializationError => "deserialize: input is malformed",
        };
        match self {
            Self::SgxError(err) => write!(f, "SGX error: {err:?}"),
            _ => f.write_str(s),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enclave::Error({self})")
    }
}

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
    /// with. Inside an enclave, get the signer with `signer`.
    pub const PROD_SIGNER: Self = Self::new(hex::decode_const(
        b"02d07f56b7f4a71d32211d6821beaeb316fbf577d02bab0dfe1f18a73de08a8e",
    ));

    /// Return the expected signer measurement by `DeployEnv` and whether
    /// we're in mock or sgx mode.
    pub const fn expected_signer(use_sgx: bool, is_dev: bool) -> Self {
        if use_sgx {
            if is_dev {
                Self::DEV_SIGNER
            } else {
                Self::PROD_SIGNER
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
    ) -> io::Result<Self> {
        let mut buf = [0u8; 4096];
        let mut digest = sha256::Context::new();

        loop {
            let n = sgxs_reader.read(&mut buf)?;
            if n == 0 {
                let hash = digest.finish();
                return Ok(Self::new(hash.to_array()));
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

lexe_byte_array::impl_byte_array!(Measurement, 32);
lexe_byte_array::impl_fromstr_fromhex!(Measurement, 32);
lexe_byte_array::impl_debug_display_as_hex!(Measurement);

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

lexe_byte_array::impl_byte_array!(MrShort, 4);
lexe_byte_array::impl_fromstr_fromhex!(MrShort, 4);
lexe_byte_array::impl_debug_display_as_hex!(MrShort);

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

    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

lexe_byte_array::impl_byte_array!(MachineId, 16);
lexe_byte_array::impl_fromstr_fromhex!(MachineId, 16);
lexe_byte_array::impl_debug_display_as_hex!(MachineId);

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

    pub fn serialize(&self) -> Vec<u8> {
        let out_len = mem::size_of::<u32>()
            + self.keyrequest.len()
            + mem::size_of::<u32>()
            + self.ciphertext.len();
        let mut out = Vec::with_capacity(out_len);

        out.extend_from_slice(&(self.keyrequest.len() as u32).to_le_bytes());
        out.extend_from_slice(self.keyrequest.as_ref());
        out.extend_from_slice(&(self.ciphertext.len() as u32).to_le_bytes());
        out.extend_from_slice(self.ciphertext.as_ref());

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
    fn read_u32_le(bytes: &[u8]) -> Result<(u32, &[u8]), Error> {
        match bytes.split_first_chunk::<4>() {
            Some((val, rest)) => Ok((u32::from_le_bytes(*val), rest)),
            None => Err(Error::DeserializationError),
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
    use proptest::{arbitrary::any, proptest, strategy::Strategy};
    use serde::de::DeserializeOwned;

    use super::*;

    #[track_caller]
    fn json_string_roundtrip<
        T: DeserializeOwned + Serialize + PartialEq + fmt::Debug,
    >(
        s1: &str,
    ) {
        let x1: T = serde_json::from_str(s1).unwrap();
        let s2 = serde_json::to_string(&x1).unwrap();
        let x2: T = serde_json::from_str(&s2).unwrap();
        assert_eq!(x1, x2);
        assert_eq!(s1, s2);
    }

    #[track_caller]
    fn fromstr_display_roundtrip<
        T: FromStr + fmt::Display + PartialEq + fmt::Debug,
    >(
        s1: &str,
    ) {
        let x1 = T::from_str(s1).map_err(|_| ()).unwrap();
        let s2 = x1.to_string();
        let x2 = T::from_str(&s2).map_err(|_| ()).unwrap();
        assert_eq!(x1, x2);
        assert_eq!(s1, s2);
    }

    #[test]
    fn serde_roundtrips() {
        json_string_roundtrip::<Measurement>(
            "\"c4f249b8d3121b0e61170a93a526beda574058f782c0b3f339e74651c379f888\"",
        );
        json_string_roundtrip::<MachineId>(
            "\"df3d290e1371112bd3da4a6cdda1f245\"",
        );
        json_string_roundtrip::<MinCpusvn>(
            "\"df3d290e1371112bd3da4a6cdda1f245\"",
        );
    }

    #[test]
    fn fromstr_display_roundtrips() {
        fromstr_display_roundtrip::<Measurement>(
            "c4f249b8d3121b0e61170a93a526beda574058f782c0b3f339e74651c379f888",
        );
        fromstr_display_roundtrip::<MachineId>(
            "df3d290e1371112bd3da4a6cdda1f245",
        );
        fromstr_display_roundtrip::<MinCpusvn>(
            "df3d290e1371112bd3da4a6cdda1f245",
        );
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
}
