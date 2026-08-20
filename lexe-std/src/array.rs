//! Small helper functions for arrays.

/// `const` concatenate two arrays.
pub const fn concat<T: Copy, const N: usize, const M: usize, const O: usize>(
    first: [T; N],
    second: [T; M],
) -> [T; O] {
    assert!(O == N + M);
    crate::const_utils::const_concat_inner(&[&first, &second])
}

/// `const` pad an `M`-byte array with zeroes, so that it's `N` bytes long.
// TODO(phlip9): should be an extension trait method, but rust doesn't allow
// const trait fns yet.
pub const fn pad<const N: usize, const M: usize>(input: [u8; M]) -> [u8; N] {
    assert!(N >= M);

    let mut out = [0u8; N];
    let mut idx = 0;
    loop {
        if idx >= M {
            break;
        }
        out[idx] = input[idx];
        idx += 1;
    }
    out
}

/// Copies of nightly-only functions for `[u8; N]`.
// TODO(phlip9): remove functions as they stabilize.
pub trait ArrayExt<const N: usize> {
    /// Divides one array reference into two at an index.
    ///
    /// The first will contain all indices from `[0, M)` (excluding
    /// the index `M` itself) and the second will contain all
    /// indices from `[M, N)` (excluding the index `N` itself).
    fn split_array_ref_stable<const M: usize>(&self) -> (&[u8; M], &[u8]);

    /// Divides one array reference into two at an index from the end.
    ///
    /// The first will contain all indices from `[0, N - M)` (excluding
    /// the index `N - M` itself) and the second will contain all
    /// indices from `[N - M, N)` (excluding the index `N` itself).
    fn rsplit_array_ref_stable<const M: usize>(&self) -> (&[u8], &[u8; M]);
}

impl<const N: usize> ArrayExt<N> for [u8; N] {
    #[inline]
    fn split_array_ref_stable<const M: usize>(&self) -> (&[u8; M], &[u8]) {
        self[..].split_first_chunk::<M>().unwrap()
    }

    #[inline]
    fn rsplit_array_ref_stable<const M: usize>(&self) -> (&[u8], &[u8; M]) {
        self[..].split_last_chunk::<M>().unwrap()
    }
}

#[cfg(test)]
mod test {
    use crate::array;

    #[test]
    fn test_concat() {
        const ACTUAL: [&str; 3] = array::concat(["a"], ["b", "c"]);
        assert_eq!(ACTUAL, ["a", "b", "c"]);

        const EMPTY: [&str; 0] = [];
        const ACTUAL_WITH_EMPTY: [&str; 3] = array::concat(EMPTY, ACTUAL);
        assert_eq!(ACTUAL_WITH_EMPTY, ACTUAL);
    }

    #[test]
    fn test_pad() {
        let input = *b"hello";
        let actual = array::pad(input);
        let expected = *b"hello\x00\x00\x00\x00\x00";
        assert_eq!(actual, expected);

        let actual = array::pad(input);
        let expected = *b"hello";
        assert_eq!(actual, expected);
    }
}
