//! A convenience module for hasing things with SHA-256.

use std::{fmt, io};

use bitcoin::secp256k1;
use ref_cast::RefCast;

use crate::{const_ref_cast, hex, hex::FromHex};

pub const HASH_LEN: usize = 32;

/// A SHA-256 Hash value.
#[derive(Copy, Clone, Default, PartialEq, Eq, RefCast)]
#[repr(transparent)]
pub struct Hash([u8; 32]);

/// A SHA-256 digest accumulator.
#[derive(Clone)]
pub struct Context(ring::digest::Context);

/// SHA-256 digest a single input.
pub fn digest(input: &[u8]) -> Hash {
    digest_many(&[input])
}

/// SHA-256 digest several input slices concatenated together, without
/// allocating.
pub fn digest_many(inputs: &[&[u8]]) -> Hash {
    let mut ctx = Context::new();
    for input in inputs {
        ctx.update(input);
    }
    ctx.finish()
}

/// SHA-256 digest a single input at compile time.
pub const fn digest_const(input: &[u8]) -> Hash {
    digest_many_const(&[input])
}

/// SHA-256 digest multiple concatenated inputs at compile time.
pub const fn digest_many_const(mut inputs: &[&[u8]]) -> Hash {
    let mut acc = sha2_const::Sha256::new();
    while let Some((input, rest)) = inputs.split_first() {
        acc = acc.update(input);
        inputs = rest;
    }
    Hash::new(acc.finalize())
}

// -- impl Hash -- //

impl Hash {
    pub const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    pub const fn from_ref(value: &[u8; 32]) -> &Self {
        const_ref_cast(value)
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub const fn as_inner(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_inner(self) -> [u8; 32] {
        self.0
    }

    // Note: not pub, since `ring::digest::Digest` is not always SHA-256, but
    // we can guarantee this invariant inside the module.
    fn from_ring(output: ring::digest::Digest) -> Self {
        Self::new(<[u8; 32]>::try_from(output.as_ref()).unwrap())
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl AsRef<[u8; 32]> for Hash {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl FromHex for Hash {
    fn from_hex(s: &str) -> Result<Self, hex::DecodeError> {
        <[u8; 32]>::from_hex(s).map(Self::new)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.as_slice()))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl secp256k1::ThirtyTwoByteHash for Hash {
    #[inline]
    fn into_32(self) -> [u8; 32] {
        self.into_inner()
    }
}

// -- impl Context -- //

impl Context {
    pub fn new() -> Self {
        Self(ring::digest::Context::new(&ring::digest::SHA256))
    }

    pub fn update(&mut self, input: &[u8]) {
        self.0.update(input);
    }

    pub fn finish(self) -> Hash {
        Hash::from_ring(self.0.finish())
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl io::Write for Context {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.update(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;

    use proptest::{arbitrary::any, proptest};

    use crate::{hex, sha256};

    // sanity check
    #[test]
    fn test_sha256() {
        let actual = hex::encode(sha256::digest(b"").as_ref());
        let expected =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(&actual, expected);
    }

    #[test]
    fn test_digest_equiv() {
        proptest!(|(inputs in any::<Vec<Vec<u8>>>())| {
            let inputs_ref = inputs
                .iter()
                .map(|v| v.as_slice())
                .collect::<Vec<_>>();

            let h1 = sha256::digest_many(&inputs_ref);
            let h2 = sha256::digest_many_const(&inputs_ref);

            let mut ctxt = sha256::Context::new();
            for input in inputs_ref {
                ctxt.write_all(input).unwrap();
            }
            let h3 = ctxt.finish();

            assert_eq!(h1, h2);
            assert_eq!(h1, h3);
        });
    }
}
