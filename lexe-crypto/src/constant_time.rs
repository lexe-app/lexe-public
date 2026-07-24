//! Constant time comparison functions.
// `ring` stopped exposing their constant time module ):

/// Constant-time `[u8; 32]` comparison function.
// https://godbolt.org/z/1GhPxrvv5
// TODO(nicole): #[cfg(target_arch = "...")] { asm!(...) } with x86_64 & aarch64
#[inline(never)]
pub fn u8x32_eq(x: &[u8; 32], y: &[u8; 32]) -> bool {
    let x00_08 = u64::from_ne_bytes(<[u8; 8]>::try_from(&x[0..8]).unwrap());
    let x08_16 = u64::from_ne_bytes(<[u8; 8]>::try_from(&x[8..16]).unwrap());
    let x16_24 = u64::from_ne_bytes(<[u8; 8]>::try_from(&x[16..24]).unwrap());
    let x24_32 = u64::from_ne_bytes(<[u8; 8]>::try_from(&x[24..32]).unwrap());

    let y00_08 = u64::from_ne_bytes(<[u8; 8]>::try_from(&y[0..8]).unwrap());
    let y08_16 = u64::from_ne_bytes(<[u8; 8]>::try_from(&y[8..16]).unwrap());
    let y16_24 = u64::from_ne_bytes(<[u8; 8]>::try_from(&y[16..24]).unwrap());
    let y24_32 = u64::from_ne_bytes(<[u8; 8]>::try_from(&y[24..32]).unwrap());

    let res = (x00_08 ^ y00_08)
        | (x08_16 ^ y08_16)
        | (x16_24 ^ y16_24)
        | (x24_32 ^ y24_32);
    res == 0
}
