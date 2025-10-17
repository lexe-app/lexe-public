//! Password-based encryption / decryption of arbitrary bytes.
//!
//! This module is a relatively thin wrapper around [`ring::pbkdf2`] which fixes
//! some parameters (algorithm choice, key stretching iterations, etc) to
//! provide a simple API for encrypting and decrypting arbitrary data under a
//! password.
//!
//! The encryption scheme is very simple:
//!
//! Encrypt:
//! - pbkdf2(password, salt) -> aes_key
//! - aes_key.encrypt(aad, data) -> ciphertext
//!
//! Decrypt:
//! - pbkdf2(password, salt) -> aes_key
//! - aes_key.decrypt(ciphertext) -> data
//!
//! The main entrypoints to this module are [`password::encrypt`] and
//! [`password::decrypt`]. See the respective function docs for details.

use std::num::NonZeroU32;

use lexe_std::const_utils;
use ring::pbkdf2;
use secrecy::Zeroize;
use thiserror::Error;

use crate::{
    aes::{self, AesMasterKey},
    rng::Crng,
};

/// The specific algorithm used for our password encryption scheme.
static PBKDF2_ALGORITHM: pbkdf2::Algorithm = pbkdf2::PBKDF2_HMAC_SHA256;
/// The number of iterations used to stretch the derived key.
/// OWASP recommends 600K iterations for PBKDF2-HMAC-SHA256:
/// <https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#pbkdf2>
const PBKDF2_ITERATIONS: NonZeroU32 =
    const_utils::const_option_unwrap(NonZeroU32::new(600_000));

/// The byte length of the secret used to construct the [`AesMasterKey`].
const AES_KEY_LEN: usize = ring::digest::SHA256_OUTPUT_LEN;

/// The minimum number of characters required in the password.
/// This is NOT the # of bytes in password (i.e. the output of [`str::len`]).
pub const MIN_PASSWORD_LENGTH: usize = 12;
/// The maximum number of characters allowed in the password.
/// This is NOT the # of bytes in password (i.e. the output of [`str::len`]).
pub const MAX_PASSWORD_LENGTH: usize = 512;
lexe_std::const_assert!(MIN_PASSWORD_LENGTH < MAX_PASSWORD_LENGTH);

#[derive(Clone, Debug, Error)]
pub enum Error {
    #[error("Password must have at least {MIN_PASSWORD_LENGTH} characters")]
    PasswordTooShort,
    #[error("Password cannot have more than {MAX_PASSWORD_LENGTH} characters")]
    PasswordTooLong,
    #[error("Decryption error: {0}")]
    AesDecrypt(#[from] aes::DecryptError),
}

/// Password-encrypt some binary `data` to a [`Vec<u8>`] ciphertext.
///
/// NOTE these requirements:
///
/// - The caller is responsible for providing a [`[u8; 32]`] `salt`, which must
///   be recoverable at decryption time. The salt should harden the user against
///   rainbow-table attacks, and must minimally be unique per-user. Ideally it
///   is unique per-user and per-service, since lots of users unfortunately
///   reuse passwords across services. The salt could also be randomly sampled
///   and persisted along with any encrypted ciphertexts for maximum security.
/// - This function does not validate that the supplied password has sufficient
///   entropy beyond enforcing a [minimum] and [maximum] length. This means that
///   "password1234", "123456123456", and "111111111111" are all valid
///   passwords. It is the responsibility of the client to enforce that the
///   given password has sufficient entropy to prevent dictionary or other
///   brute-force attacks.
///
/// [minimum]: MIN_PASSWORD_LENGTH
/// [maximum]: MAX_PASSWORD_LENGTH
pub fn encrypt(
    rng: &mut impl Crng,
    password: &str,
    salt: &[u8; 32],
    data: &[u8],
) -> Result<Vec<u8>, Error> {
    validate_password_len(password)?;

    // Derive the AES key using PBKDF2.
    let aes_key = derive_aes_key(password, salt);

    // Encrypt the data under the derived AES key, using the salt as the AAD.
    let aad = &[salt.as_slice()];
    let data_size_hint = Some(data.len());
    // We don't expose write_data_cb as a parameter bc AFAICT we won't be
    // password-encrypting anything which must first be serialized into bytes.
    let write_data_cb = |buf: &mut Vec<u8>| buf.extend_from_slice(data);
    let ciphertext = aes_key.encrypt(rng, aad, data_size_hint, &write_data_cb);

    Ok(ciphertext)
}

/// Given a `password`, `salt`, and some `ciphertext`, decrypts the ciphertext.
pub fn decrypt(
    password: &str,
    salt: &[u8; 32],
    ciphertext: Vec<u8>,
) -> Result<Vec<u8>, Error> {
    // OK to validate length here because we check for backwards compat in tests
    validate_password_len(password)?;

    // Derive the AES key using PBKDF2.
    let aes_key = derive_aes_key(password, salt);

    // Decrypt, using the salt as the AAD.
    let aad = &[salt.as_slice()];
    let data = aes_key.decrypt(aad, ciphertext)?;

    Ok(data)
}

/// Validate the length of the given password which the caller intends to use
/// for password encryption. We don't check that the password has enough
/// entropy; this should be done by the client.
pub fn validate_password_len(password: &str) -> Result<(), Error> {
    let password_length = password.chars().count();
    if password_length < MIN_PASSWORD_LENGTH {
        return Err(Error::PasswordTooShort);
    }
    if password_length > MAX_PASSWORD_LENGTH {
        return Err(Error::PasswordTooLong);
    }
    Ok(())
}

/// Given a password and salt, use PBKDF2 to derive an [`AesMasterKey`] which
/// can be used to encrypt or decrypt data.
fn derive_aes_key(password: &str, salt: &[u8; 32]) -> AesMasterKey {
    let mut aes_key_buf = [0u8; AES_KEY_LEN];
    pbkdf2::derive(
        PBKDF2_ALGORITHM,
        PBKDF2_ITERATIONS,
        salt,
        password.as_bytes(),
        &mut aes_key_buf,
    );
    let aes_key = AesMasterKey::new(&aes_key_buf);
    // Ensure AES key seed bytes are zeroized.
    aes_key_buf.zeroize();
    aes_key
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use proptest::{
        arbitrary::any, proptest, strategy::Strategy, test_runner::Config,
    };
    use secrecy::ExposeSecret;

    use super::*;
    use crate::{rng::FastRng, root_seed::RootSeed};

    #[test]
    fn encryption_roundtrip() {
        // Reduce cases since we do key stretching which is quite expensive
        let config = Config::with_cases(4);
        let password_length_range = MIN_PASSWORD_LENGTH..MAX_PASSWORD_LENGTH;
        let any_valid_password =
            proptest::collection::vec(any::<char>(), password_length_range)
                .prop_map(String::from_iter);
        proptest!(config, |(
            mut rng in any::<FastRng>(),
            password in any_valid_password,
            salt in any::<[u8; 32]>(),
            data1 in any::<Vec<u8>>(),
        )| {
            let ciphertext =
                encrypt(&mut rng, &password, &salt, &data1).unwrap();
            let data2 = decrypt(&password, &salt, ciphertext).unwrap();
            assert_eq!(data1, data2);
        })
    }

    /// Tests that updates to the decryption algorithm are backwards-compatible.
    #[test]
    fn decryption_compatibility() {
        // Set `maybe_ciphertext` to `None` to regenerate
        struct TestCase {
            password: String,
            salt: [u8; 32],
            data1: &'static [u8],
            maybe_ciphertext: Option<&'static str>,
        }

        // Case 0: Medium-length password with all zero salt and empty data
        let case0 = TestCase {
            password: "medium-length!123123".to_owned(),
            salt: [0u8; 32],
            data1: b"",
            maybe_ciphertext: Some(
                "00a9ebf955ed070fe7acefe66e5a007b2c4165d3c2c23efc6a91d60a37e3a7b6181e4156d15d513cb9cee00739a226466e",
            ),
        };
        // Case 1: Minimum-length password as of 2023-10-16 (12 chars)
        let case1 = TestCase {
            password: "passwordword".to_owned(),
            salt: [69; 32],
            data1: b"*jaw drops* awooga! hummina hummina bazooing!",
            maybe_ciphertext: Some(
                "00a9ebf955ed070fe7acefe66e5a007b2c4165d3c2c23efc6a91d60a37e3a7b6180c0d3cd90616335f13f5de7c9df0a1d89a7aec282b8083089c2360962e22db1a57685e82aea236c053b88495021767e0c17e05b3f72a86cfbbffc3724a",
            ),
        };
        // Case 2: Maximum-length password as of 2023-10-16 (512 chars)
        let password = (0u32..512)
            .map(|i| char::from_u32(i).unwrap())
            .collect::<String>();
        let case2 = TestCase {
            password,
            salt: [69; 32],
            data1: b"*jaw drops* awooga! hummina hummina bazooing!",
            maybe_ciphertext: Some(
                "00a9ebf955ed070fe7acefe66e5a007b2c4165d3c2c23efc6a91d60a37e3a7b618cf7a8ff3ea628ed33fb32428930340557454454258dedc67c9a3a5e350c2408ad82e6a8ac02779fd9df3f513364b6351301271cfd2c515fdca0cd15de0",
            ),
        };

        for (i, case) in [case0, case1, case2].into_iter().enumerate() {
            let TestCase {
                password,
                salt,
                data1,
                maybe_ciphertext,
            } = case;

            match maybe_ciphertext {
                Some(cipherhext) => {
                    // Test decryption of ciphertext
                    println!("Testing case {i}");
                    let ciphertext = hex::decode(cipherhext).unwrap();
                    let data2 = decrypt(&password, &salt, ciphertext).unwrap();
                    assert_eq!(data1, data2.as_slice());
                }
                None => {
                    // Generate and print the ciphertext to build the test case
                    let mut rng = FastRng::from_u64(20231016);
                    let ciphertext =
                        encrypt(&mut rng, &password, &salt, data1).unwrap();
                    let cipherhext = hex::display(&ciphertext);
                    println!("Case {i} ciphertext: {cipherhext}");
                }
            }
        }
    }

    // ```bash
    // $ nix shell .#secretctl
    // $ PASSWORD=".." IN_PATH=".." \
    //     cargo test -p common --lib -- test_decrypt_root_seed --nocapture --ignored
    // ```
    #[test]
    #[ignore]
    fn test_decrypt_root_seed() {
        let password = std::env::var("PASSWORD").expect("`$PASSWORD` not set");
        let in_path = std::env::var_os("IN_PATH").expect("`$IN_PATH` not set");
        let in_path = Path::new(&in_path);

        let ciphertext = std::fs::read(in_path).unwrap();
        let root_seed = RootSeed::password_decrypt(&password, ciphertext)
            .expect("Failed to decrypt");

        let root_seed_bytes = root_seed.expose_secret().as_slice();
        let mut root_seed_hex = hex::encode(root_seed_bytes);
        println!("{root_seed_hex}");

        root_seed_hex.zeroize();
    }
}
