//! Form field validators.

use common::password;
use flutter_rust_bridge::frb;
use secrecy::Zeroize;

/// Validate whether `password` has an appropriate length.
///
/// The return type is a bit funky: `Option<String>`. `None` means
/// `address_str` is valid, while `Some(msg)` means it is not (with given
/// error message). We return in this format to better match the flutter
/// `FormField` validator API.
#[frb(sync)]
pub fn validate_password(mut password: String) -> Option<String> {
    let result = password::validate_password_len(&password);
    password.zeroize();
    match result {
        Ok(()) => None,
        Err(err) => Some(err.to_string()),
    }
}
