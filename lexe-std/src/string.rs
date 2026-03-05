//! String utilities.

/// Truncates a [`String`] to at most `max_bytes` bytes, ensuring the result
/// is valid UTF-8 by finding the nearest character boundary.
///
/// If `s.len() <= max_bytes`, this is a no-op.
///
/// Assumption: data in input distribution is almost always shorter than
/// `max_bytes`.
#[inline(always)]
pub fn truncate_bytes(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    truncate_bytes_cold(s, max_bytes)
}

/// Truncates a [`String`] to at most `max_chars` characters.
///
/// If the string has fewer than `max_chars` characters, this is a no-op.
///
/// Assumption: data in input distribution is almost always shorter than
/// `max_chars`.
#[inline(always)]
pub fn truncate_chars(s: &mut String, max_chars: usize) {
    if s.len() <= max_chars {
        return;
    }
    truncate_chars_cold(s, max_chars)
}

#[inline(never)]
#[cold]
fn truncate_bytes_cold(s: &mut String, max_bytes: usize) {
    // UTF-8 code points are 1-4 bytes long, so we can limit our search to this
    // range: [max_bytes - 3, max_bytes]
    for idx in (max_bytes.saturating_sub(3)..=max_bytes).rev() {
        if s.is_char_boundary(idx) {
            s.truncate(idx);
            break;
        }
    }
}

#[inline(never)]
#[cold]
fn truncate_chars_cold(s: &mut String, max_chars: usize) {
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;

    let bytes = s.as_bytes();
    let len = bytes.len();
    let (chunks, _) = bytes.as_chunks::<8>();

    let mut idx = 0usize;
    let mut chars_seen = 0usize;

    for chunk in chunks {
        let word = u64::from_ne_bytes(*chunk);

        // Continuation bytes are `10xxxxxx`: bit7=1 and bit6=0.
        let continuation_mask = (word & HIGH_BITS) & !((word << 1) & HIGH_BITS);
        let continuation_count = continuation_mask.count_ones() as usize;
        let chunk_chars = 8usize - continuation_count;

        // Accept the whole 8-byte chunk only if it keeps us within MAX_CHARS;
        // otherwise fall back to byte-wise refinement from the current `idx`.
        chars_seen += chunk_chars;
        if chars_seen > max_chars {
            chars_seen -= chunk_chars;
            break;
        }

        idx += 8;
    }

    while idx < len {
        if (bytes[idx] & 0b1100_0000) != 0b1000_0000 {
            chars_seen += 1;
            if chars_seen > max_chars {
                // `idx` is a non-continuation byte, so it is a UTF-8 scalar
                // boundary and therefore a valid truncate index.
                s.truncate(idx);
                return;
            }
        }
        idx += 1;
    }
}

#[cfg(test)]
mod tests {
    use proptest::{prop_assert, prop_assert_eq, proptest};

    use super::*;

    // Helper: truncate a clone by bytes and return the result.
    fn tb(s: &str, max_bytes: usize) -> String {
        let mut s = s.to_owned();
        truncate_bytes(&mut s, max_bytes);
        s
    }

    // Helper: truncate a clone by chars and return the result.
    fn tc(s: &str, max_chars: usize) -> String {
        let mut s = s.to_owned();
        truncate_chars(&mut s, max_chars);
        s
    }

    #[test]
    fn test_truncate_bytes() {
        // No-ops: empty, under limit, at limit
        assert_eq!(tb("", 10), "");
        assert_eq!(tb("hello", 10), "hello");
        assert_eq!(tb("hello", 5), "hello");

        // ASCII truncation
        assert_eq!(tb("hello world", 5), "hello");

        // Multibyte: "a😀b" = 1 + 4 + 1 = 6 bytes
        // Cutting at 3 lands inside the emoji; backs up to byte 1.
        assert_eq!(tb("a\u{1F600}b", 3), "a");

        // CJK: 3 bytes each. "日本語" = 9 bytes.
        assert_eq!(tb("日本語", 7), "日本"); // mid-char backs up
        assert_eq!(tb("日本語", 6), "日本"); // exact boundary
    }

    #[test]
    fn test_truncate_chars() {
        // No-ops: empty, under limit, at limit
        assert_eq!(tc("", 10), "");
        assert_eq!(tc("hello", 10), "hello");
        assert_eq!(tc("hello", 5), "hello");

        // ASCII truncation
        assert_eq!(tc("hello world", 5), "hello");

        // Multibyte: "a😀b😀c" = 5 chars
        assert_eq!(tc("a\u{1F600}b\u{1F600}c", 3), "a\u{1F600}b");

        // CJK: "日本語テスト" = 6 chars
        assert_eq!(tc("日本語テスト", 3), "日本語");

        // Zero chars
        assert_eq!(tc("hello", 0), "");
    }

    // Both truncate_bytes and truncate_chars are idempotent.
    // ∀ f ∈ {tb, tc}, s, n.  f(s, n) = f(f(s, n), n)
    #[test]
    fn test_truncate_idempotent() {
        proptest!(|(s: String, n in 0usize..=512)| {
            let bytes_once = tb(&s, n);
            let bytes_twice = tb(&bytes_once, n);
            prop_assert_eq!(bytes_once, bytes_twice);

            let chars_once = tc(&s, n);
            let chars_twice = tc(&chars_once, n);
            prop_assert_eq!(chars_once, chars_twice);
        });
    }

    // For the same length, truncating by chars will be longer than truncating
    // by bytes.
    // ∀ s, n.  s.len() >= tc(s, n).len() >= tb(s, n).len()
    #[test]
    fn test_truncate_length_ordering() {
        proptest!(|(s: String, n in 0usize..=512)| {
            let original_len = s.len();
            let chars_len = tc(&s, n).len();
            let bytes_len = tb(&s, n).len();

            prop_assert!(original_len >= chars_len);
            prop_assert!(chars_len >= bytes_len);
        });
    }

    // Truncating to the original length / num chars after appending any ASCII
    // byte / char gives the original string.
    // ∀ s, b.  s == tb(s || b, s.len())
    // ∀ s, c.  s == tc(s || c, s.chars().count())
    #[test]
    fn test_truncate_prefix_recovery() {
        proptest!(|(s: String, ascii in 0u8..=0x7f, c: char)| {
            let mut with_ascii = s.clone();
            with_ascii.push(char::from(ascii));
            let bytes_recovered = tb(&with_ascii, s.len());
            prop_assert_eq!(bytes_recovered, s.clone());

            let mut with_char = s.clone();
            with_char.push(c);
            let chars_recovered = tc(&with_char, s.chars().count());
            prop_assert_eq!(chars_recovered, s);
        });
    }
}
