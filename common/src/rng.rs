//! Random number generation utilities

use std::num::NonZeroU32;

use bitcoin::secp256k1::{All, Secp256k1};
#[cfg(all(any(test, feature = "test-utils")))]
use proptest::{
    arbitrary::{any, Arbitrary},
    strategy::{BoxedStrategy, Strategy},
};
use rand_core::le::read_u32_into;
pub use rand_core::{CryptoRng, RngCore, SeedableRng};
use ring::rand::SecureRandom;

const RAND_ERROR_CODE: NonZeroU32 =
    NonZeroU32::new(rand_core::Error::CUSTOM_START).unwrap();

/// Helper to get a `secp256k1` context randomized for side-channel resistance.
/// Use this function instead of calling [`Secp256k1::new`] directly.
pub fn get_randomized_secp256k1_ctx<R: Crng>(rng: &mut R) -> Secp256k1<All> {
    let mut random_buf = [0u8; 32];
    rng.fill_bytes(&mut random_buf);
    let mut secp_ctx = Secp256k1::new();
    secp_ctx.seeded_randomize(&random_buf);
    secp_ctx
}

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
pub struct WeakRng {
    s0: u32,
    s1: u32,
}

impl WeakRng {
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

impl Default for WeakRng {
    fn default() -> Self {
        Self::new()
    }
}

/// Only enable [`CryptoRng`] for this rng when testing.
#[cfg(any(test, feature = "test-utils"))]
impl CryptoRng for WeakRng {}

impl RngCore for WeakRng {
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

impl SeedableRng for WeakRng {
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

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for WeakRng {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // We use `no_shrink` here since shrinking an RNG seed won't produce
        // "simpler" output samples. This setting lets `proptest` know not to
        // waste time trying to shrink the rng seed.
        any::<[u8; 8]>()
            .no_shrink()
            .prop_map(WeakRng::from_seed)
            .boxed()
    }
}

/// Map `x` uniformly into the range `[0, n)`. Has slight modulo bias for large
/// ranges.
///
/// See: <https://lemire.me/blog/2016/06/27/a-fast-alternative-to-the-modulo-reduction/>
#[cfg(any(test, feature = "test-utils"))]
#[inline(always)]
const fn fastmap32(x: u32, n: u32) -> u32 {
    let mul = (x as u64).wrapping_mul(n as u64);
    (mul >> 32) as u32
}

/// Shuffle a slice. Very fast, but has slight modulo bias so don't use for
/// crypto.
#[cfg(any(test, feature = "test-utils"))]
pub fn shuffle<T, R>(rng: &mut R, xs: &mut [T])
where
    R: RngCore,
{
    assert!(xs.len() < (u32::MAX as usize));

    for i in (1..xs.len()).rev() {
        // invariant: elements with index > i have been locked in place.
        let n = (i as u32) + 1;
        let j = fastmap32(rng.next_u32(), n) as usize;
        xs.swap(i, j);
    }
}
