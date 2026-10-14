//! HKDF-SHA256 key derivation.
//!
//! A light wrapper around [`ring::hkdf`].

use std::fmt;

/// An HKDF-SHA256 pseudorandom key, from which [`Prk::expand`] derives keys.
pub struct Prk(ring::hkdf::Prk);

impl Prk {
    /// The most bytes a single [`Prk::expand`] can output.
    pub const MAX_OUT_LEN: usize = 255 * 32;

    /// HKDF-Extract a pseudorandom key from `ikm` and `salt`.
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Self {
        let salt = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, salt);
        Self(salt.extract(ikm))
    }

    /// HKDF-Expand `info` into `out`.
    ///
    /// Panics if `out` is longer than [`Prk::MAX_OUT_LEN`].
    pub fn expand(&self, info: &[&[u8]], out: &mut [u8]) {
        struct OkmLength(usize);

        impl ring::hkdf::KeyType for OkmLength {
            fn len(&self) -> usize {
                self.0
            }
        }

        assert!(out.len() <= Self::MAX_OUT_LEN);
        self.0
            .expand(info, OkmLength(out.len()))
            .expect("Output length is checked above")
            .fill(out)
            .expect("Output length matches")
    }
}

impl fmt::Debug for Prk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Prk(..)")
    }
}

#[cfg(test)]
mod test {
    use lexe_hex::hex;

    use super::*;

    /// RFC 5869, A.1. Test Case 1
    #[test]
    fn rfc5869_test_case_1() {
        let ikm = [0x0b; 22];
        let salt = hex::decode("000102030405060708090a0b0c").unwrap();
        let info = hex::decode("f0f1f2f3f4f5f6f7f8f9").unwrap();

        let mut okm = [0u8; 42];
        Prk::extract(&salt, &ikm).expand(&[&info[..5], &info[5..]], &mut okm);
        assert_eq!(
            hex::encode(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db0\
             2d56ecc4c5bf34007208d5b887185865",
        );
    }
}
