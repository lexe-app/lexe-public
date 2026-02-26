//! String utilities.

/// Truncates a [`String`] to at most `max_bytes` bytes, ensuring the result
/// is valid UTF-8 by finding the nearest character boundary.
///
/// If `s.len() <= max_bytes`, this is a no-op.
pub fn truncate_bytes(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }

    // Move back to the nearest UTF-8 boundary (at most 3 iterations for
    // 4-byte chars).
    // TODO(a-mpch): use nightly `str::floor_char_boundary` when stable.
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }

    s.truncate(end);
}

/// Truncates a [`String`] to at most `max_chars` characters.
///
/// If the string has fewer than `max_chars` characters, this is a no-op.
pub fn truncate_chars(s: &mut String, max_chars: usize) {
    // Find the byte index where the max_chars-th character starts.
    if let Some((byte_idx, _)) = s.char_indices().nth(max_chars) {
        s.truncate(byte_idx);
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
