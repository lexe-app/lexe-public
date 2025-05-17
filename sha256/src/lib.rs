//! A convenience crate for hasing things with SHA-256.

use std::io;

use byte_array::ByteArray;
use ref_cast::RefCast;

pub const HASH_LEN: usize = 32;

/// A SHA-256 Hash value.
#[derive(Copy, Clone, Default, Eq, Hash, PartialEq, RefCast)]
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

// -- impl Hash -- //

impl Hash {
    pub const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    // Note: not pub, since `ring::digest::Digest` is not always SHA-256, but
    // we can guarantee this invariant inside the module.
    fn from_ring(output: ring::digest::Digest) -> Self {
        Self::new(<[u8; 32]>::try_from(output.as_ref()).unwrap())
    }
}

byte_array::impl_byte_array!(Hash, 32);
byte_array::impl_fromstr_fromhex!(Hash, 32);
byte_array::impl_debug_display_as_hex!(Hash);

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
    use crate as sha256;

    // sanity check
    #[test]
    fn test_sha256() {
        let actual = hex::encode(sha256::digest(b"").as_ref());
        let expected =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(&actual, expected);
    }
}
