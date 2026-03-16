//! Lexe [`Crng`] compatibility hacks to support LDK's [`EntropySource`].
//!
//! [`Crng`]: lexe_crypto::rng::Crng
//! [`EntropySource`]: lightning::sign::EntropySource
//
// These types live in `lexe-common` to avoid including any `lightning`
// dependencies in our low-level, foundational crates, as `lightning` takes a
// long time to compile.

#[cfg(any(test, feature = "test-utils"))]
use lexe_crypto::rng::FastRng;
use lexe_crypto::rng::{RngExt, SysRng};
use lightning::sign::EntropySource;

/// Dumb hack so we can pass `SysRng` as an LDK [`EntropySource`] without
/// wrapping in an Arc/Box.
#[repr(transparent)]
pub struct SysRngDerefHack(InnerSysrng);

impl SysRngDerefHack {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(InnerSysrng)
    }
}

impl std::ops::Deref for SysRngDerefHack {
    type Target = InnerSysrng;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[doc(hidden)] // needs to be `pub` for `Deref<Target = InnerSysrng>`
pub struct InnerSysrng;

impl EntropySource for InnerSysrng {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        SysRng::new().gen_bytes()
    }
}

/// Dumb hack so we can use a `FastRng` as an [`EntropySource`].
///
/// Note that because LDK requires `&self` for [`EntropySource`], this will
/// return the same randomness for every `get_secure_random_bytes` call after
/// creation.
#[cfg(any(test, feature = "test-utils"))]
#[repr(transparent)]
pub struct FastRngDerefHack(InnerFastRng);

#[cfg(any(test, feature = "test-utils"))]
impl FastRngDerefHack {
    pub fn from_u64(seed: u64) -> Self {
        Self(InnerFastRng(FastRng::from_u64(seed)))
    }

    pub fn from_rng(rng: &mut FastRng) -> Self {
        let rng = FastRng::from_u64(rng.gen_u64());
        Self(InnerFastRng(rng))
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl std::ops::Deref for FastRngDerefHack {
    type Target = InnerFastRng;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)] // needs to be `pub` for `Deref<Target = InnerFastRng>`
pub struct InnerFastRng(FastRng);

#[cfg(any(test, feature = "test-utils"))]
impl EntropySource for InnerFastRng {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.0.clone().gen_bytes()
    }
}
