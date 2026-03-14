use std::sync::LazyLock;

use bitcoin::secp256k1::{All, Secp256k1};

use crate::rng::{RngExt, SysRng};

/// A global static [`Secp256k1`] context to avoid creating multiple contexts.
///
/// Suitable for both signing and signature verification. Use this function
/// instead of calling [`Secp256k1::new`] directly.
///
/// This context is automatically randomized using [`SysRng`] for
/// defense-in-depth against side-channel attacks.
//
// * Each ctx randomize is at least one ecmult, which is fairly expensive. It's
//   nice to only do this once.
// * It turns out All vs SignOnly vs VerifyOnly does nothing in libsecp256k1
//   anymore, so just don't bother.
// * Using this global context also doesn't significantly impact test
//   determinism, since signing is deterministic. It would only maybe trip up a
//   coverage-guided fuzzer.
pub static SECP256K1: LazyLock<Secp256k1<All>> = LazyLock::new(|| {
    #[allow(clippy::disallowed_methods)]
    let mut ctx = Secp256k1::new();
    ctx.seeded_randomize(&SysRng::new().gen_bytes());
    ctx
});
