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

pub fn seal<'a>(
    rng: &mut dyn Crng,
    label: &[u8],
    data: &[u8], // TODO(phlip9): allow sealing in-place
) -> Result<Sealed<'a>, Error> {
    #[cfg(not(target_env = "sgx"))]
    let result = mock::seal(rng, label, data);

    #[cfg(target_env = "sgx")]
    let result = sgx::seal(rng, label, data);

    result
}

pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    #[cfg(not(target_env = "sgx"))]
    let result = mock::unseal(label, sealed);

    #[cfg(target_env = "sgx")]
    let result = sgx::unseal(label, sealed);

    result
}
