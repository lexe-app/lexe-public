//! Interface over in-enclave APIs. Dummy data is provided outside enclaves
//! for debugging.

#![allow(dead_code)]

#[cfg(target_env = "sgx")]
mod sgx;

#[cfg(not(target_env = "sgx"))]
mod mock;

use std::{borrow::Cow, fmt, io, mem, str::FromStr};

use bytes::{Buf, BufMut};
use cfg_if::cfg_if;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    env::DeployEnv,
    hex::{self, FromHex},
    hexstr_or_bytes,
    rng::Crng,
    sha256,
};

pub const MOCK_MEASUREMENT: Measurement =
    Measurement::new(*b"~~~~~~~ LEXE MOCK ENCLAVE ~~~~~~");

pub const MOCK_SIGNER: Measurement =
    Measurement::new(*b"======= LEXE MOCK SIGNER =======");

/// The enclave signer measurement our debug enclaves are signed with.
/// This is also the measurement of the fortanix/rust-sgx dummy key:
/// <https://github.com/fortanix/rust-sgx/blob/master/intel-sgx/enclave-runner/src/dummy.key>
///
/// Running an enclave with `run-sgx .. --debug` will automatically sign with
/// this key just before running.
pub const DEV_SIGNER: Measurement = Measurement::new(hex::decode_const(
    b"9affcfae47b848ec2caf1c49b4b283531e1cc425f93582b36806e52a43d78d1a",
));

/// The enclave signer measurement our production enclaves should be signed
/// with. Inside an enclave, retrieve the signer with
/// [`enclave::signer()`](signer).
pub const PROD_SIGNER: Measurement = Measurement::new(hex::decode_const(
    b"02d07f56b7f4a71d32211d6821beaeb316fbf577d02bab0dfe1f18a73de08a8e",
));

// TODO(phlip9): use the machine id of my dev machine until we build a proper
//               get-machine-id bin util.
pub const MOCK_MACHINE_ID: MachineId =
    MachineId::new(hex::decode_const(b"52bc575eb9618084083ca7b3a45a2a76"));
// pub const MOCK_MACHINE_ID: MachineId = MachineId::new(*b"!MOCK MACHINE ID");

/// In SGX enclaves, this is the current CPUSVN we commit to when
/// sealing data.
///
/// Updated: 2024/01/03 - Linux SGX platform v2.21
pub const MIN_SGX_CPUSVN: MinCpusvn =
    MinCpusvn::new(hex::decode_const(b"0c0c100fffff01000000000000000000"));

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

impl From<sgx_isa::ErrorCode> for Error {
    fn from(err: sgx_isa::ErrorCode) -> Self {
        Self::SgxError(err)
    }
}

/// An enclave measurement.
///
/// Get the current enclave's measurement with
/// [`enclave::measurement()`](measurement).
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub struct Measurement(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// A [`Measurement`] shortened to its first four bytes (8 hex chars).
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub struct MrShort(#[serde(with = "hexstr_or_bytes")] [u8; 4]);

impl Measurement {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn into_inner(self) -> [u8; 32] {
        self.0
    }

    pub fn as_inner(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn short(&self) -> MrShort {
        MrShort::from(self)
    }
}

impl FromStr for Measurement {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for Measurement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.0.as_slice()))
    }
}

impl fmt::Debug for Measurement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl MrShort {
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Whether this [`MrShort`] is a prefix of the given [`Measurement`].
    fn is_prefix_of(&self, long: &Measurement) -> bool {
        self.0 == long.0[..4]
    }
}

impl From<&Measurement> for MrShort {
    fn from(long: &Measurement) -> Self {
        (long.0)[..4].try_into().map(Self).unwrap()
    }
}

impl FromStr for MrShort {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 4]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for MrShort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.0.as_slice()))
    }
}

impl fmt::Debug for MrShort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// A unique identifier for a particular hardware enclave.
///
/// The intention with this identifier is to compactly communicate whether a
/// piece of hardware with this id (in its current state) has the _capability_
/// to [`unseal`] some [`Sealed`] data that was [`seal'ed`](seal) on hardware
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
#[derive(Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MachineId(#[serde(with = "hexstr_or_bytes")] [u8; 16]);

impl MachineId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

impl FromStr for MachineId {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 16]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(&self.0))
    }
}

impl fmt::Debug for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MachineId")
            .field(&hex::display(&self.0))
            .finish()
    }
}

#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MinCpusvn(#[serde(with = "hexstr_or_bytes")] [u8; 16]);

impl MinCpusvn {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn inner(self) -> [u8; 16] {
        self.0
    }
}

impl FromStr for MinCpusvn {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 16]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for MinCpusvn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(&self.0))
    }
}

impl fmt::Debug for MinCpusvn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MinCpusvn")
            .field(&hex::display(&self.0))
            .finish()
    }
}

/// Sealed and encrypted data
#[derive(PartialEq, Eq)]
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

// TODO(phlip9): use a real serialization format like CBOR or something

fn read_u32_le(mut bytes: &[u8]) -> Result<(u32, &[u8]), Error> {
    if bytes.len() >= mem::size_of::<u32>() {
        Ok((bytes.get_u32_le(), bytes))
    } else {
        Err(Error::DeserializationError)
    }
}

fn read_bytes(bytes: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    let (len, bytes) = read_u32_le(bytes)?;
    let len = len as usize;
    if bytes.len() >= len {
        Ok(bytes.split_at(len))
    } else {
        Err(Error::DeserializationError)
    }
}

impl<'a> Sealed<'a> {
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
        let (keyrequest, bytes) = read_bytes(bytes)?;
        let (ciphertext, bytes) = read_bytes(bytes)?;

        if bytes.is_empty() {
            Ok(Self {
                keyrequest: Cow::Borrowed(keyrequest),
                ciphertext: Cow::Borrowed(ciphertext),
            })
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

// TODO(phlip9): additional authenticated data?

/// Seal and encrypt data in an enclave so that it's only readable inside
/// another enclave running the same software.
///
/// Users should also provide a unique domain-separation `label` for each unique
/// sealing type or location.
///
/// Data is currently encrypted with AES-256-GCM using the [`ring`] backend.
///
/// In SGX, this sealed data is only readable by other enclave instances with
/// the exact same [`MRENCLAVE`] measurement. The sealing key also commits to
/// the platform [CPUSVN], meaning enclaves running on platforms with
/// out-of-date SGX TCB will be unable to unseal data sealed on updated
/// SGX platforms.
///
/// SGX sealing keys are sampled uniquely and only used to encrypt data once. In
/// effect, the `keyid` is a nonce but the key itself is only deriveable inside
/// an enclave with an exactly matching [`MRENCLAVE`] (among other things).
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
            sgx::seal(rng, label, data)
        } else {
            mock::seal(rng, label, data)
        }
    }
}

/// Unseal and decrypt data previously sealed with [`seal`].
///
/// See [`seal`] for more details.
pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            sgx::unseal(label, sealed)
        } else {
            mock::unseal(label, sealed)
        }
    }
}

/// Return the current enclave measurement.
///
/// + In SGX, this is often called the [`MRENCLAVE`].
///
/// + In mock mode, this returns a fixed value.
///
/// + The enclave measurement is a SHA-256 hash summary of the enclave code and
///   initial memory contents.
///
/// + This hash uniquely identifies an enclave; any change to the code will also
///   change the measurement.
///
/// [`MRENCLAVE`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#enclave-measurement-mrenclave
pub fn measurement() -> Measurement {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            sgx::measurement()
        } else {
            mock::measurement()
        }
    }
}

/// Retrieve the enclave signer measurement of the current running enclave.
/// Every enclave is signed with an RSA keypair, plus some extra metadata.
///
/// + In SGX, this value is called the [`MRSIGNER`].
///
/// + In mock mode, this returns a fixed value.
///
/// + The signer is a 3072-bit RSA keypair with exponent=3. The signer
///   measurement is the SHA-256 hash of the (little-endian) public key modulus.
///
/// [`MRSIGNER`]: https://phlip9.com/notes/confidential%20computing/intel%20SGX/SGX%20lingo/#signer-measurement-mrsigner
pub fn signer() -> Measurement {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            sgx::signer()
        } else {
            mock::signer()
        }
    }
}

/// Return the expected signer measurement by [`DeployEnv`] and whether we're in
/// mock or sgx mode.
pub const fn expected_signer(use_sgx: bool, env: DeployEnv) -> Measurement {
    if use_sgx {
        match env {
            DeployEnv::Prod | DeployEnv::Staging => PROD_SIGNER,
            DeployEnv::Dev => DEV_SIGNER,
        }
    } else {
        MOCK_SIGNER
    }
}

/// Compute an enclave measurement from an `.sgxs` file stream
/// [`std::io::Read`].
///
/// * Enclave binaries are first converted to `.sgxs` files, which exactly
///   mirror the memory layout of the loaded enclave binaries right before
///   running.
///
/// * Conveniently, the SHA-256 hash of an enclave `.sgxs` binary is exactly the
///   same as the actual enclave measurement hash, since the memory layout is
///   identical (caveat: unless we use some more sophisticated extendable
///   enclave features).
pub fn compute_measurement<R: io::Read>(
    mut sgxs_reader: R,
) -> io::Result<Measurement> {
    let mut buf = [0u8; 4096];
    let mut digest = sha256::Context::new();

    loop {
        let n = sgxs_reader.read(&mut buf)?;
        if n == 0 {
            let hash = digest.finish();
            return Ok(Measurement::new(hash.into_inner()));
        } else {
            digest.update(&buf[0..n]);
        }
    }
}

/// Get the current machine id from inside an enclave.
///
/// See: [`MachineId`]
pub fn machine_id() -> MachineId {
    cfg_if! {
        if #[cfg(target_env = "sgx")] {
            sgx::machine_id()
        } else {
            mock::machine_id()
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assume, proptest, strategy::Strategy};

    use super::*;
    use crate::{rng::WeakRng, test_utils::roundtrip};

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
    fn test_sealing_roundtrip_basic() {
        let mut rng = WeakRng::new();

        let sealed = seal(&mut rng, b"", b"".as_slice().into()).unwrap();
        let unsealed = unseal(b"", sealed).unwrap();
        assert_eq!(&unsealed, b"");

        let sealed =
            seal(&mut rng, b"cool label", b"cool data".as_slice().into())
                .unwrap();
        let unsealed = unseal(b"cool label", sealed).unwrap();
        assert_eq!(&unsealed, b"cool data");
    }

    #[test]
    fn test_sealing_roundtrip_proptest() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();

        proptest!(|(mut rng in any::<WeakRng>(), label in arb_label, data in arb_data)| {
            let sealed = seal(&mut rng, &label, data.clone().into()).unwrap();
            let unsealed = unseal(&label, sealed).unwrap();
            assert_eq!(&data, &unsealed);
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
    fn test_sealing_detects_ciphertext_change() {
        let arb_label = any::<Vec<u8>>();
        let arb_data = any::<Vec<u8>>();
        let arb_mutation = any::<Vec<u8>>()
            .prop_filter("can't be empty or all zeroes", |m| {
                !m.is_empty() && !m.iter().all(|x| x == &0u8)
            });

        proptest!(|(
            mut rng in any::<WeakRng>(),
            label in arb_label,
            data in arb_data,
            mutation in arb_mutation,
        )| {
            let sealed = seal(&mut rng, &label, data.into()).unwrap();

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
            unseal(&label, sealed).unwrap_err();
        });
    }

    #[test]
    fn test_measurement_consistent() {
        let m1 = measurement();
        let m2 = measurement();
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_machine_id_consistent() {
        let m1 = machine_id();
        let m2 = machine_id();
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

    // TODO(phlip9): test KeyRequest mutations
    // TODO(phlip9): test truncate/extend mutations
}
