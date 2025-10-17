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
