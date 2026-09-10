//! Securely generate a `[u8; 32]` seed from a CSPRNG (typically `SysRng` ->
//! `ring::rand::SystemRng` -> `getrandom`), while mixing in additional
//! easily-accessible entropy to add safety margin.
//!
//! ### Use case
//!
//! We're currently targeting this module just for `RootSeed` generation. The
//! `RootSeed` is the long-lived root secret for Lexe users and can't be
//! rotated.
//!
//! ### Threat model
//!
//! Since `RootSeed`s are generated on user devices using their OS's platform
//! CSPRNG, we don't know how good that CSPRNG actually is. Modern user
//! desktops, phones, and servers generally have solid CSPRNGs, but it's not
//! impossible for there to be a bug or misconfiguration or rare platform
//! that leads to lower quality randomness.
//!
//! Our main goal here is to add some safety margin in the event that the user's
//! platform RNG is buggy or partially ineffective. Ideally we have enough
//! margin that affected users have some time to move funds.
//!
//! Regardless, as a system, we still depend on a secure and reliable system
//! RNG.
//!
//! ### Comparison
//!
//! The gold standard for paranoid user-space entropy collection is probably
//! Bitcoin Core's
//! [`randomenv.cpp`](https://github.com/bitcoin/bitcoin/blob/master/src/randomenv.cpp).
//!
//! This impl follows a similar structure, though we intentionally avoid all
//! non-trivial I/O and avoid any dependencies outside `std` to reduce
//! complexity.

#[cfg(not(target_env = "sgx"))]
use std::process;
use std::{
    hash::Hash,
    hint, ptr,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use lexe_byte_array::ByteArray;
use lexe_sha256::sha256;
use lexe_std::array;
use zeroize::Zeroizing;

use crate::rng::{Crng, RngExt};

/// Securely sample a `[u8; 32]` seed from a CSPRNG and mixin in additional
/// easily accessible entropy to add safety margin.
pub fn gen_seed(rng: &mut impl Crng) -> [u8; 32] {
    const DOMAIN_SEPARATOR: [u8; 32] = array::pad(*b"LEXE-REALM::gen_seed");

    let mut ctx = sha256::Context::new();
    ctx.update(&DOMAIN_SEPARATOR);

    // Collect timestamps before and after to also pick up some small timing
    // jitter entropy.
    source::unix_timestamp_ns(&mut ctx);
    source::monotonic_time(&mut ctx);

    source::rng_bytes(&mut ctx, rng);
    source::counter(&mut ctx);
    source::code_addr(&mut ctx);
    source::static_addr(&mut ctx);
    source::stack_addr(&mut ctx);
    source::heap_addr(&mut ctx, rng);
    source::thread_local_addr(&mut ctx);
    #[cfg(not(target_env = "sgx"))]
    source::process_id(&mut ctx);
    source::thread_id(&mut ctx);

    source::unix_timestamp_ns(&mut ctx);
    source::monotonic_time(&mut ctx);

    ctx.finish().to_array()
}

/// Individual entropy sources
mod source {
    use super::*;

    /// Mixin 256B from system CSPRNG.
    pub fn rng_bytes(ctx: &mut sha256::Context, rng: &mut impl Crng) {
        let mut bytes = Zeroizing::new([0u8; 256]);
        rng.fill_bytes(&mut *bytes);
        ctx.update(bytes.as_slice());
    }

    /// Mixin wall-clock ns since Unix epoch.
    pub fn unix_timestamp_ns(ctx: &mut sha256::Context) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        ctx.update(&timestamp.to_ne_bytes());
    }

    /// Mixin system monotonic clock input from [`Instant`].
    pub fn monotonic_time(ctx: &mut sha256::Context) {
        // Use `std::hash::Hash` since `Instant` repr is intentionally opaque.
        Instant::now().hash(ctx);
    }

    /// Mixin process-wide counter to separate multiple in-process calls.
    pub fn counter(ctx: &mut sha256::Context) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        ctx.update(&counter.to_ne_bytes());
    }

    /// Mixin an address in the .code section. On Linux, this yields ~28b of
    /// entropy from ASLR.
    pub fn code_addr(ctx: &mut sha256::Context) {
        // TODO(phlip9): use `std::marker::FntPtr::addr()` from nightly when
        // that stabilizes.
        let addr = (code_addr as *const ()).addr();
        ctx.update(&addr.to_ne_bytes());
    }

    /// Mixin an address in the .data/.rodata section.
    pub fn static_addr(ctx: &mut sha256::Context) {
        static VALUE: u8 = 0;
        let addr = ptr::from_ref(hint::black_box(&VALUE)).addr();
        ctx.update(&addr.to_ne_bytes());
    }

    /// Mixin an address in the stack. On Linux, may yield ~20b of entropy from
    /// ASLR.
    pub fn stack_addr(ctx: &mut sha256::Context) {
        let value = 0u8;
        let addr = ptr::from_ref(hint::black_box(&value)).addr();
        ctx.update(&addr.to_ne_bytes());
    }

    /// Mixin a heap address from a random 1-1024B allocation. On Linux, may
    /// yield ~10-20b of entropy from ASLR, plus more from allocation history
    /// and the particular arena the allocation ends up in.
    pub fn heap_addr(ctx: &mut sha256::Context, rng: &mut impl Crng) {
        let len = rng.gen_range_u32(1..1025) as usize;
        let value = hint::black_box(Vec::<u8>::with_capacity(len));
        let addr = value.as_ptr().addr();
        ctx.update(&addr.to_ne_bytes());
    }

    /// Mixin an address in thread-local storage. Likely correlated with stack
    /// address, but depends on the system.
    pub fn thread_local_addr(ctx: &mut sha256::Context) {
        thread_local! {
            // clippy errors when built for SGX without without this lint line
            // TODO(phlip9): incorrect lint, remove when clippy not broken
            #[allow(clippy::missing_const_for_thread_local)]
            static VALUE: u8 = const { 0 };
        }

        VALUE.with(|value| {
            let addr = ptr::from_ref(hint::black_box(value)).addr();
            ctx.update(&addr.to_ne_bytes());
        });
    }

    /// Mixin the current process ID. Maybe a few bits of entropy.
    #[cfg(not(target_env = "sgx"))]
    pub fn process_id(ctx: &mut sha256::Context) {
        ctx.update(&process::id().to_ne_bytes());
    }

    /// Mixin the current thread ID. Maybe a bit or two of entropy, depending on
    /// the # of threads spawned.
    pub fn thread_id(ctx: &mut sha256::Context) {
        // Use `std::hash::Hash` since `ThreadId` repr is intentionally opaque.
        thread::current().id().hash(ctx);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rng::SysRng;

    #[test]
    fn test_gen_seed() {
        let mut rng = SysRng::new();
        assert_ne!(gen_seed(&mut rng), gen_seed(&mut rng));
    }

    #[test]
    fn test_sources() {
        let sources: &[fn(&mut sha256::Context)] = &[
            source::unix_timestamp_ns,
            source::monotonic_time,
            source::counter,
            source::code_addr,
            source::static_addr,
            source::stack_addr,
            |ctx| source::heap_addr(ctx, &mut SysRng::new()),
            source::thread_local_addr,
            #[cfg(not(target_env = "sgx"))]
            source::process_id,
            source::thread_id,
        ];
        let empty = sha256::Context::new().finish();
        for &source in sources {
            assert_ne!(digest_source(source), empty);
        }
    }

    #[test]
    fn test_counter() {
        assert_ne!(
            digest_source(source::counter),
            digest_source(source::counter)
        );
    }

    fn digest_source(source: fn(&mut sha256::Context)) -> sha256::Hash {
        let mut ctx = sha256::Context::new();
        source(&mut ctx);
        ctx.finish()
    }
}
