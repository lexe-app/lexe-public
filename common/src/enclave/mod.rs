//! Interface over in-enclave APIs. Dummy data is provided outside enclaves
//! for debugging.

#![allow(dead_code)]

#[cfg(target_env = "sgx")]
mod sgx;

#[cfg(not(target_env = "sgx"))]
mod mock;

use std::borrow::Cow;

use thiserror::Error;

use crate::rng::Crng;

#[derive(Debug, Error)]
#[error("error")]
pub enum Error {
    #[error("SGX error: {0:?}")]
    SgxError(sgx_isa::ErrorCode),

    #[error("temp")]
    Other,
}

impl From<sgx_isa::ErrorCode> for Error {
    fn from(err: sgx_isa::ErrorCode) -> Self {
        Self::SgxError(err)
    }
}

impl From<ring::error::Unspecified> for Error {
    fn from(_: ring::error::Unspecified) -> Self {
        Self::Other
    }
}

// platform specific APIs
//
// 1. seal/unseal
// 2. self report
// 3. report for targetinfo
// 4. verify report mac

pub struct Sealed<'a> {
    /// A truncated key request
    keyrequest: Cow<'a, [u8]>,
    /// Include the sealing enclave's attributes and miscselect flags so we can
    /// tell that some sealed data won't be unsealable (rather than a wrong key
    /// error).
    attributes: [u64; 2],
    miscselect: u32,
    /// Encrypted ciphertext
    ciphertext: Cow<'a, [u8]>,
}

pub fn seal<'a>(
    rng: &mut dyn Crng,
    label: [u8; 16],
    data: &[u8], // TODO(phlip9): allow sealing in-place
) -> Result<Sealed<'a>, Error> {
    #[cfg(target_env = "sgx")]
    let result = sgx::seal(rng, label, data);

    #[cfg(not(target_env = "sgx"))]
    let result = mock::seal(rng, label, data);

    result
}

pub fn unseal(_label: [u8; 16], _sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    Err(Error::Other)
}
