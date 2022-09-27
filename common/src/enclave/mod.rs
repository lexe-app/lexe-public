//! Interface over in-enclave APIs. Dummy data is provided outside enclaves
//! for debugging.

#![allow(dead_code)]

#[cfg(target_env = "sgx")]
mod sgx;

#[cfg(not(target_env = "sgx"))]
mod mock;

use std::borrow::Cow;
use std::str::FromStr;
use std::{fmt, mem};

use bytes::{Buf, BufMut};
use cfg_if::cfg_if;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hex::{self, FromHex};
use crate::rng::Crng;

pub const MOCK_MEASUREMENT: Measurement =
    Measurement::new(*b"~~~~~~~ LEXE MOCK ENCLAVE ~~~~~~");

// TODO(phlip9): use the machine id of my dev machine until we build a proper
//               get-machine-id bin util.
pub const MOCK_MACHINE_ID: MachineId =
    MachineId::new(hex::decode_const(b"52bc575eb9618084083ca7b3a45a2a76"));
// pub const MOCK_MACHINE_ID: MachineId = MachineId::new(*b"!MOCK MACHINE ID");

/// In SGX enclaves, this is the current CPUSVN we commit to when
/// sealing data.
pub const MIN_SGX_CPUSVN: MinCpusvn =
    MinCpusvn::new(hex::decode_const(b"08080e0dffff01000000000000000000"));

#[derive(Debug, Error)]
#[error("error")]
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
#[derive(Copy, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
pub struct Measurement(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

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
        write!(f, "{}", self)
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
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
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
    use proptest::arbitrary::any;
    use proptest::strategy::Strategy;
    use proptest::{prop_assume, proptest};

    use super::*;
    use crate::rng::{arb_rng, SmallRng};

    #[test]
    fn test_sealing_roundtrip_basic() {
        let mut rng = SmallRng::new();

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

        proptest!(|(mut rng in arb_rng(), label in arb_label, data in arb_data)| {
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
            mut rng in arb_rng(),
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

    // TODO(phlip9): test KeyRequest mutations
    // TODO(phlip9): test truncate/extend mutations
}
