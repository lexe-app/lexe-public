//! Random number generation utilities

use std::{cell::Cell, num::NonZeroU32};

use bitcoin::secp256k1::{All, Secp256k1, SignOnly};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{
    arbitrary::{any, Arbitrary},
    strategy::{BoxedStrategy, Strategy},
};
use rand_core::le::read_u32_into;
pub use rand_core::{CryptoRng, RngCore, SeedableRng};
use ring::rand::SecureRandom;

const RAND_ERROR_CODE: NonZeroU32 = const_utils::const_option_unwrap(
    NonZeroU32::new(rand_core::Error::CUSTOM_START),
);

/// A succinct trait alias for a Cryptographically Secure PRNG. Includes a few
/// utility methods for security-critical random value generation.
pub trait Crng: RngCore + CryptoRng {
    /// Helper to get a `secp256k1` context randomized for side-channel
    /// resistance. Suitable for both signing and signature verification.
    /// Use this function instead of calling [`Secp256k1::new`] directly.
    fn gen_secp256k1_ctx(&mut self) -> Secp256k1<All>;

    /// Helper to get a `secp256k1` context randomized for side-channel
    /// resistance. This context can only sign, not verify.
    /// Use this function instead of calling [`Secp256k1::new`] directly.
    fn gen_secp256k1_ctx_signing(&mut self) -> Secp256k1<SignOnly>;
}

impl<R: RngCore + CryptoRng> Crng for R {
    fn gen_secp256k1_ctx(&mut self) -> Secp256k1<All> {
        #[allow(clippy::disallowed_methods)]
        let mut ctx = Secp256k1::new();
        ctx.seeded_randomize(&self.gen_bytes());
        ctx
    }

    fn gen_secp256k1_ctx_signing(&mut self) -> Secp256k1<SignOnly> {
        #[allow(clippy::disallowed_methods)]
        let mut ctx = Secp256k1::signing_only();
        ctx.seeded_randomize(&self.gen_bytes());
        ctx
    }
}

/// Minimal extension trait on [`rand_core::RngCore`], containing small utility
/// methods for generating random values.
pub trait RngExt: RngCore {
    fn gen_bytes<const N: usize>(&mut self) -> [u8; N];
    fn gen_bool(&mut self) -> bool;
    fn gen_u8(&mut self) -> u8;
    fn gen_u16(&mut self) -> u16;
    fn gen_u32(&mut self) -> u32;
    fn gen_u64(&mut self) -> u64;
    fn gen_u128(&mut self) -> u128;
}

impl<R: RngCore> RngExt for R {
    fn gen_bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        self.fill_bytes(&mut out);
        out
    }

    fn gen_bool(&mut self) -> bool {
        let byte: [u8; 1] = self.gen_bytes();
        byte[0] & 0x1 == 0
    }

    #[inline]
    fn gen_u8(&mut self) -> u8 {
        u8::from_le_bytes(self.gen_bytes())
    }

    #[inline]
    fn gen_u16(&mut self) -> u16 {
        u16::from_le_bytes(self.gen_bytes())
    }

    #[inline]
    fn gen_u32(&mut self) -> u32 {
        self.next_u32()
    }

    #[inline]
    fn gen_u64(&mut self) -> u64 {
        self.next_u64()
    }

    #[inline]
    fn gen_u128(&mut self) -> u128 {
        u128::from_le_bytes(self.gen_bytes())
    }
}

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

impl lightning::sign::EntropySource for SysRng {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        self.0.fill(&mut out).expect("ring SystemRandom failed");
        out
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
#[cfg_attr(any(test, feature = "test-utils"), derive(Clone))]
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
        let (s0, s1, r) = xoroshiro64star_next_u32(self.s0, self.s1);
        self.s0 = s0;
        self.s1 = s1;
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

/// The core rng step that generates the next random output for [`WeakRng`] and
/// [`ThreadWeakRng`].
#[inline(always)]
fn xoroshiro64star_next_u32(mut s0: u32, mut s1: u32) -> (u32, u32, u32) {
    let r = s0.wrapping_mul(0x9e3779bb);
    s1 ^= s0;
    s0 = s0.rotate_left(26) ^ s1 ^ (s1 << 9);
    s1 = s1.rotate_left(13);
    (s0, s1, r)
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
impl lightning::sign::EntropySource for WeakRng {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.clone().gen_bytes()
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

/// A thread-local [`WeakRng`] that is seeded from the global [`SysRng`] the
/// first time a thread uses it.
///
/// Like `WeakRng`, it's a small, fast, and _non-cryptographic_ rng with decent
/// statistical properties. Useful for sampling non-security sensitive data.
///
/// Shines in multithreaded/async scenarios where don't want to have to
/// synchronize on a single `Mutex<WeakRng>` or deal with handing out `WeakRng`s
/// to each thread. Instead we let thread-locals handle all the drudgery.
pub struct ThreadWeakRng(());

impl ThreadWeakRng {
    #[inline]
    pub fn new() -> Self {
        Self(())
    }
}

impl Default for ThreadWeakRng {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Only enable [`CryptoRng`] for this rng when testing.
#[cfg(any(test, feature = "test-utils"))]
impl CryptoRng for ThreadWeakRng {}

thread_local! {
    // Can't put a `WeakRng` here directly, since it's not `Copy`
    // (and shouldn't impl `Copy`).
    //
    // Using `const { .. }` with a noop-drop type (allegedly) lets us
    // use a faster thread_local impl.
    static THREAD_RNG_STATE: Cell<u64> = const { Cell::new(0) };
}

impl RngCore for ThreadWeakRng {
    fn next_u32(&mut self) -> u32 {
        let mut s01 = THREAD_RNG_STATE.get();

        // Need to seed this thread-local rng
        if s01 == 0 {
            // Mark this branch cold to encourage better codegen, since
            // seeding should only happen once per thread.
            #[cold]
            #[inline(never)]
            fn reseed() -> u64 {
                SysRng::new().gen_u64()
            }
            s01 = reseed();
        }

        // unpack state
        let s0 = (s01 >> 32) as u32;
        let s1 = s01 as u32;

        // gen next state and output
        let (s0, s1, r) = xoroshiro64star_next_u32(s0, s1);

        // repack state and update
        let s01 = ((s0 as u64) << 32) | (s1 as u64);
        THREAD_RNG_STATE.set(s01);

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
