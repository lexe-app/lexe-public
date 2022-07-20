#![allow(dead_code)]

use std::borrow::Cow;

use crate::enclave::{Error, Sealed};
use crate::rng::Crng;

pub fn seal(
    _rng: &mut dyn Crng,
    _label: &[u8],
    _data: Cow<'_, [u8]>,
) -> Result<Sealed<'static>, Error> {
    // TODO(phlip9): impl
    Err(Error::SealInputTooLarge)
}

pub fn unseal(_label: &[u8], _sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    // TODO(phlip9): impl
    Err(Error::UnsealDecryptionError)
}
