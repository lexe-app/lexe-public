//! Form field validators.

use common::password;
use secrecy::Zeroize;

/// Validate whether `password` has an appropriate length.
///
/// The return type is a bit funky: `Option<String>`. `None` means
/// `address_str` is valid, while `Some(msg)` means it is not (with given
/// error message). We return in this format to better match the flutter
/// `FormField` validator API.
///
/// flutter_rust_bridge:sync
pub fn validate_password(mut password: String) -> Option<String> {
    let result = password::validate_password_len(&password);
    password.zeroize();
    match result {
        Ok(()) => None,
        Err(err) => Some(err.to_string()),
    }
}

/// flutter_rust_bridge:sync
pub fn suggest_mnemonic_words(prefix: &str, take: usize) -> Vec<String> {
    bip39::Language::English
        .words_by_prefix(prefix)
        .iter()
        .take(take)
        .copied()
        .map(|w| w.to_owned())
        .collect()
}

/// flutter_rust_bridge:sync
pub fn is_mnemonic_word(word: &str) -> bool {
    bip39::Language::English.find_word(word).is_some()
}

/// Parse a raw mnemonic phrase from clipboard text.
///
/// Returns `Ok(Vec<String>)` with exactly 24 valid BIP-39 words,
/// or an error with a user-friendly message.
///
/// flutter_rust_bridge:sync
// TODO(a-mpch): Remove `allow(dead_code)` once FFI codegen exposes this
// function.
#[allow(dead_code)]
pub fn parse_mnemonic_phrase(raw: String) -> anyhow::Result<Vec<String>> {
    let lang = bip39::Language::English;
    let is_number = |t: &str| t.trim_end_matches('.').parse::<u32>().is_ok();

    // Preprocess: filter out numbered prefixes, normalize to lowercase.
    let preprocessed = raw
        .split_whitespace()
        .filter(|t| !is_number(t))
        .map(|t| t.trim_end_matches('.').to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    // Parse and validate (word count, valid words, checksum).
    let mnemonic = bip39::Mnemonic::parse_in_normalized(lang, &preprocessed)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(mnemonic.words().map(|w| w.to_owned()).collect())
}

#[cfg(test)]
mod test {
    use super::*;

    // A valid 24-word BIP-39 mnemonic for testing.
    const VALID_WORDS: [&str; 24] = [
        "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
        "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
        "abandon", "abandon", "abandon", "abandon", "abandon", "abandon",
        "abandon", "abandon", "abandon", "abandon", "abandon", "art",
    ];

    #[test]
    fn parse_space_separated() {
        let input = VALID_WORDS.join(" ");
        assert_eq!(parse_mnemonic_phrase(input).unwrap(), VALID_WORDS);
    }

    #[test]
    fn parse_various_whitespace() {
        // Tabs, newlines, extra spaces, uppercase - all normalized.
        let input = format!(
            "  ABANDON\t{}\n{}  ",
            VALID_WORDS[1..12].join("\t"),
            VALID_WORDS[12..24].join("\n"),
        );
        assert_eq!(parse_mnemonic_phrase(input).unwrap(), VALID_WORDS);
    }

    #[test]
    fn parse_numbered_format() {
        // Legacy format: "1. word1 2. word2 ..."
        let input = VALID_WORDS
            .iter()
            .enumerate()
            .map(|(i, w)| format!("{}. {}", i + 1, w))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(parse_mnemonic_phrase(input).unwrap(), VALID_WORDS);
    }

    #[test]
    fn error_invalid_input() {
        // Invalid word count (5 words is not a valid BIP-39 length).
        let err =
            parse_mnemonic_phrase(VALID_WORDS[0..5].join(" ")).unwrap_err();
        assert!(err.to_string().contains("invalid"));

        // Invalid word.
        let mut words = VALID_WORDS.to_vec();
        words[5] = "notaword";
        let err = parse_mnemonic_phrase(words.join(" ")).unwrap_err();
        assert!(err.to_string().contains("unknown word"));
    }
}
