//! Interface over in-enclave APIs. Dummy data is provided outside enclaves
//! for debugging.

#![allow(dead_code)]

#[cfg(target_env = "sgx")]
mod sgx;

#[cfg(not(target_env = "sgx"))]
mod mock;

use std::borrow::Cow;
use std::fmt;

use thiserror::Error;

use crate::hex;
use crate::rng::Crng;

/// In SGX enclaves, this is the current CPUSVN we commit to when
/// sealing data.
pub const MIN_SGX_CPUSVN: [u8; 16] =
    hex::decode_const(b"08080e0dffff01000000000000000000");

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
}

impl From<sgx_isa::ErrorCode> for Error {
    fn from(err: sgx_isa::ErrorCode) -> Self {
        Self::SgxError(err)
    }
}

/// Sealed and encrypted data
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
    #[cfg(not(target_env = "sgx"))]
    let result = mock::seal(rng, label, data);

    #[cfg(target_env = "sgx")]
    let result = sgx::seal(rng, label, data);

    result
}

/// Unseal and decrypt data previously sealed with [`seal`].
///
/// See [`seal`] for more details.
pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    #[cfg(not(target_env = "sgx"))]
    let result = mock::unseal(label, sealed);

    #[cfg(target_env = "sgx")]
    let result = sgx::unseal(label, sealed);

    result
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
pub fn measurement() -> [u8; 32] {
    #[cfg(not(target_env = "sgx"))]
    let result = mock::measurement();

    #[cfg(target_env = "sgx")]
    let result = sgx::measurement();

    result
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

    // TODO(phlip9): test KeyRequest mutations
    // TODO(phlip9): test truncate/extend mutations
}
