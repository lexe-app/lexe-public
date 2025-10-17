//! Random number generation utilities

use std::{cell::Cell, num::NonZeroU32};

use bitcoin::secp256k1::{All, Secp256k1, SignOnly};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{
    arbitrary::{Arbitrary, any},
    strategy::{BoxedStrategy, Strategy},
};
pub use rand::Rng;
use rand::prelude::Distribution;
use rand_core::le::read_u32_into;
pub use rand_core::{CryptoRng, RngCore, SeedableRng};
use ring::rand::SecureRandom;

const RAND_ERROR_CODE: NonZeroU32 =
    NonZeroU32::new(rand_core::Error::CUSTOM_START).unwrap();

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
pub trait RngExt: RngCore + Rng {
    fn gen_bytes<const N: usize>(&mut self) -> [u8; N];
    fn gen_boolean(&mut self) -> bool;
    fn gen_u8(&mut self) -> u8;
    fn gen_u16(&mut self) -> u16;
    fn gen_u32(&mut self) -> u32;
    fn gen_u64(&mut self) -> u64;
    fn gen_u128(&mut self) -> u128;
    fn gen_f32(&mut self) -> f32;
    fn gen_f64(&mut self) -> f64;

    /// Generate `N` (nearly uniformly random) alphanumeric (0-9, A-Z, a-z)
    /// bytes.
    fn gen_alphanum_bytes<const N: usize>(&mut self) -> [u8; N];
}

impl<R: RngCore + Rng> RngExt for R {
    fn gen_bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        self.fill_bytes(&mut out);
        out
    }

    fn gen_alphanum_bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = self.gen_bytes();
        encode_alphanum_bytes(&mut out);
        out
    }

    /// Named `gen_boolean` to avoid ambiguity with [`Rng::gen_bool`].
    fn gen_boolean(&mut self) -> bool {
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

    /// Generates a [`f32`] uniformly distributed in `[0, 1)`.
    #[inline]
    fn gen_f32(&mut self) -> f32 {
        rand::distributions::Standard.sample(self)
    }

    /// Generates a [`f64`] uniformly distributed in `[0, 1)`.
    #[inline]
    fn gen_f64(&mut self) -> f64 {
        rand::distributions::Standard.sample(self)
    }
}

#[inline(never)]
fn encode_alphanum_bytes<const N: usize>(inout: &mut [u8; N]) {
    for x in inout.iter_mut() {
        *x = encode_alphanum_byte(*x);
    }
}

/// "project" a full byte `x ∈ [0, 255]` into the alphanumeric ASCII character
/// range `(['0','9'] ∪ ['A','Z'] ∪ ['a','z'])`.
///
/// The projection is slightly biased (e.g., P('0') = 5/256 vs P('1') = 4/256),
/// to avoid a rejection sampling loop and improve codegen.
#[inline(always)]
#[allow(non_snake_case)]
const fn encode_alphanum_byte(x: u8) -> u8 {
    //                    idx >= 10               idx >= 10 + 26
    //                         |                       |
    //                         v                       v
    // [         ] -- gap9A -- [         ] -- gapZa -- [         ]
    // 0 1 2 ... 9 : ; ... ? @ A B ... Y Z ] \ ... _ ` a b ... y z

    let idx = fastmap8(x, 10 + 26 + 26);

    let base = idx + b'0';
    let gap_9A = if idx >= 10 { b'A' - b'9' - 1 } else { 0 };
    let gap_Za = if idx >= 10 + 26 { b'a' - b'Z' - 1 } else { 0 };

    base + gap_9A + gap_Za
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

/// Dumb hack so we can pass `SysRng` as an `EntropySource` without wrapping
/// in an Arc/Box.
#[repr(transparent)]
pub struct SysRngDerefHack(pub SysRng);

impl SysRngDerefHack {
    pub fn new() -> Self {
        Self(SysRng::new())
    }
}

impl Default for SysRngDerefHack {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for SysRngDerefHack {
    type Target = SysRng;
    fn deref(&self) -> &Self::Target {
        &self.0
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
pub struct FastRng {
    s0: u32,
    s1: u32,
}

impl FastRng {
    pub fn new() -> Self {
        Self {
            s0: 0xdeadbeef,
            s1: 0xf00baa44,
        }
    }

    /// Seed a new [`FastRng`] from an existing [`SysRng`].
    pub fn from_sysrng(sys_rng: &mut SysRng) -> Self {
        let seed = sys_rng.gen_u64();
        Self::seed_from_u64(seed)
    }

    pub fn from_u64(s: u64) -> Self {
        Self::seed_from_u64(s)
    }
}

impl Default for FastRng {
    fn default() -> Self {
        Self::new()
    }
}

/// Only enable [`CryptoRng`] for this rng when testing.
#[cfg(any(test, feature = "test-utils"))]
impl CryptoRng for FastRng {}

impl RngCore for FastRng {
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

/// The core rng step that generates the next random output for [`FastRng`] and
/// [`ThreadFastRng`].
#[inline(always)]
fn xoroshiro64star_next_u32(mut s0: u32, mut s1: u32) -> (u32, u32, u32) {
    let r = s0.wrapping_mul(0x9e3779bb);
    s1 ^= s0;
    s0 = s0.rotate_left(26) ^ s1 ^ (s1 << 9);
    s1 = s1.rotate_left(13);
    (s0, s1, r)
}

impl SeedableRng for FastRng {
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
impl lightning::sign::EntropySource for FastRng {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.clone().gen_bytes()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for FastRng {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // We use `no_shrink` here since shrinking an RNG seed won't produce
        // "simpler" output samples. This setting lets `proptest` know not to
        // waste time trying to shrink the rng seed.
        any::<[u8; 8]>()
            .no_shrink()
            .prop_map(FastRng::from_seed)
            .boxed()
    }
}

/// A thread-local [`FastRng`] that is seeded from the global [`SysRng`] the
/// first time a thread uses it.
///
/// Like `FastRng`, it's a small, fast, and _non-cryptographic_ rng with decent
/// statistical properties. Useful for sampling non-security sensitive data.
///
/// Shines in multithreaded/async scenarios where don't want to have to
/// synchronize on a single `Mutex<FastRng>` or deal with handing out `FastRng`s
/// to each thread. Instead we let thread-locals handle all the drudgery.
pub struct ThreadFastRng(());

impl ThreadFastRng {
    #[inline]
    pub fn new() -> Self {
        Self(())
    }

    /// Set the current thread local rng seed.
    pub fn seed(seed: u64) {
        // splitmix64
        // <https://github.com/rust-random/rngs/blob/master/rand_xoshiro/src/splitmix64.rs#L48>
        let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z = z ^ (z >> 31);
        THREAD_RNG_STATE.set(z)
    }
}

impl Default for ThreadFastRng {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Only enable [`CryptoRng`] for this rng when testing.
#[cfg(any(test, feature = "test-utils"))]
impl CryptoRng for ThreadFastRng {}

// Can't put a `FastRng` here directly, since it's not `Copy`
// (and shouldn't impl `Copy`).
//
// Using `const { .. }` with a noop-drop type (allegedly) lets us
// use a faster thread_local impl.
thread_local! {
    // clippy errors when built for SGX without without this lint line
    // TODO(phlip9): incorrect lint, remove when clippy not broken
    #[allow(clippy::missing_const_for_thread_local)]
    static THREAD_RNG_STATE: Cell<u64> = const { Cell::new(0) };
}

impl RngCore for ThreadFastRng {
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
    ((x as u64).wrapping_mul(n as u64) >> 32) as u32
}

/// Map `x` uniformly into the range `[0, n)`. Has slight modulo bias for large
/// ranges.
///
/// See: <https://lemire.me/blog/2016/06/27/a-fast-alternative-to-the-modulo-reduction/>
#[inline(always)]
const fn fastmap8(x: u8, n: u8) -> u8 {
    ((x as u16).wrapping_mul(n as u16) >> 8) as u8
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

#[cfg(test)]
mod test {
    use proptest::{prop_assert, proptest};

    use super::*;

    #[test]
    fn test_encode_alphanum_byte() {
        let mut mset = [0u8; 256];
        for c in 0..=255 {
            let o = encode_alphanum_byte(c);
            mset[o as usize] += 1;
        }

        let actual_alphabet = mset
            .as_slice()
            .iter()
            .enumerate()
            .filter(|(_idx, count)| **count != 0)
            .map(|(idx, _count)| (idx as u8) as char)
            .collect::<String>();

        let expected_alphabet =
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        assert_eq!(&actual_alphabet, expected_alphabet);
        assert_eq!(actual_alphabet.len(), 10 + 26 + 26);
    }

    #[test]
    fn test_gen_alphanum_bytes() {
        proptest!(|(mut rng: FastRng)| {
            let alphanum = rng.gen_alphanum_bytes::<16>();
            let alphanum_str = std::str::from_utf8(alphanum.as_slice()).unwrap();
            prop_assert!(alphanum_str.chars().all(|c| c.is_ascii_alphanumeric()));
        });
    }
}
