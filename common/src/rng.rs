//! Random number generation utilities

use std::num::NonZeroU32;

#[cfg(all(any(test, feature = "test-utils")))]
use proptest::arbitrary::{any, Arbitrary};
#[cfg(all(any(test, feature = "test-utils")))]
use proptest::strategy::{BoxedStrategy, Strategy};
use rand_core::le::read_u32_into;
pub use rand_core::{CryptoRng, RngCore, SeedableRng};
use ring::rand::SecureRandom;

const RAND_ERROR_CODE: NonZeroU32 =
    NonZeroU32::new(rand_core::Error::CUSTOM_START).unwrap();

/// A succinct trait alias for a Cryptographically Secure PRNG.
pub trait Crng: RngCore + CryptoRng {}

impl<R: RngCore + CryptoRng> Crng for R {}

/// A compatibility wrapper so we can use `ring`'s PRG with `rand` traits.
#[derive(Clone, Debug)]
pub struct SysRng(ring::rand::SystemRandom);

impl SysRng {
    pub fn new() -> Self {
        Self(ring::rand::SystemRandom::new())
    }
}

impl Default for SysRng {
    fn default() -> Self {
        Self::new()
    }
}

/// [`ring::rand::SystemRandom`] is a cryptographically secure PRG
impl CryptoRng for SysRng {}

impl RngCore for SysRng {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        rand_core::impls::next_u32_via_fill(self)
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        rand_core::impls::next_u64_via_fill(self)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.try_fill_bytes(dest).expect("ring SystemRandom failed")
    }

    fn try_fill_bytes(
        &mut self,
        dest: &mut [u8],
    ) -> Result<(), rand_core::Error> {
        self.0
            .fill(dest)
            // just some random error code. ring's error type here is
            // empty/unspecified anyway, so not a big deal.
            .map_err(|_| rand_core::Error::from(RAND_ERROR_CODE))
    }
}

/// A small, fast, _non-cryptographic_ rng with decent statistical properties.
/// Useful for sampling non-security sensitive data or as a deterministic RNG
/// for tests (instead of the [`SysRng`] above, which uses the global OS RNG).
///
/// The implementation is the same as [`Xoroshiro64Star`].
///
/// [`Xoroshiro64Star`]: https://github.com/rust-random/rngs/blob/master/rand_xoshiro/src/xoroshiro64star.rs
#[derive(Debug)]
pub struct SmallRng {
    s0: u32,
    s1: u32,
}

impl SmallRng {
    pub fn new() -> Self {
        Self {
            s0: 0xdeadbeef,
            s1: 0xf00baa44,
        }
    }

    pub fn from_u64(s: u64) -> Self {
        Self::seed_from_u64(s)
    }
}

impl Default for SmallRng {
    fn default() -> Self {
        Self::new()
    }
}

/// Only enable [`CryptoRng`] for this rng when testing.
#[cfg(any(test, feature = "test-utils"))]
impl CryptoRng for SmallRng {}

impl RngCore for SmallRng {
    #[inline]
    fn next_u32(&mut self) -> u32 {
        let r = self.s0.wrapping_mul(0x9e3779bb);
        self.s1 ^= self.s0;
        self.s0 = self.s0.rotate_left(26) ^ self.s1 ^ (self.s1 << 9);
        self.s1 = self.s1.rotate_left(13);
        r
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        rand_core::impls::next_u64_via_u32(self)
    }

    #[inline]
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        rand_core::impls::fill_bytes_via_next(self, dest);
    }

    #[inline]
    fn try_fill_bytes(
        &mut self,
        dest: &mut [u8],
    ) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl SeedableRng for SmallRng {
    type Seed = [u8; 8];

    fn from_seed(seed: Self::Seed) -> Self {
        // zero is a pathological case for Xoroshiro64Star, just map it to
        // the default seed
        if seed == [0u8; 8] {
            Self::new()
        } else {
            let mut parts = [0u32, 0u32];
            read_u32_into(&seed, &mut parts);
            Self {
                s0: parts[0],
                s1: parts[1],
            }
        }
    }
}

#[cfg(all(any(test, feature = "test-utils")))]
impl Arbitrary for SmallRng {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // We use `no_shrink` here since shrinking an RNG seed won't produce
        // "simpler" output samples. This setting lets `proptest` know not to
        // waste time trying to shrink the rng seed.
        any::<[u8; 8]>()
            .no_shrink()
            .prop_map(SmallRng::from_seed)
            .boxed()
    }
}
