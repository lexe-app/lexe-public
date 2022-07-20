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
    /// A truncated [`KeyRequest`](crate::enclave::sgx::KeyRequest)
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
