#![allow(dead_code)]

use crate::enclave::{Error, Sealed};
use crate::rng::Crng;

pub fn seal(
    _rng: &mut dyn Crng,
    _label: [u8; 16],
    _data: &[u8],
) -> Result<Sealed<'static>, Error> {
    Err(Error::Other)
}

pub fn unseal(_label: [u8; 16], _sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    Err(Error::Other)
}
