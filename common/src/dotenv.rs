use std::env;

/// A version of [`dotenvy::dotenv`] which only loads whitelisted keys.
///
/// # Safety
///
/// This fn calls [`std::env::set_var`] under-the-hood, which is not
/// threadsafe on some platforms (ex: glibc Linux). The caller must ensure that
/// this fn is called very early in the program lifetime, before any threads are
/// spawned.
pub unsafe fn dotenv_filtered(
    filter_keys: &[&str],
) -> Result<(), dotenvy::Error> {
    // `dotenv_iter` finds an .env file in the current directory (or parents),
    // then returns an iterator over it without loading the variables within.
    for try_kv in dotenvy::dotenv_iter()? {
        let (key, value) = try_kv?;
        if filter_keys.contains(&key.as_str()) {
            // Like dotenvy::dotenv(), do not override existing keys.
            if env::var(&key).is_err() {
                // See SAFETY
                unsafe {
                    env::set_var(&key, value);
                }
            }
        }
    }

    Ok(())
}

/// Fetches the given key from env or `.env`, but without loading any of the
/// keys in `.env` into our env (including the requested key).
/// In other words, it is a "pure" version of [`dotenvy::var`].
pub fn var_pure(given_key: &str) -> Result<String, dotenvy::Error> {
    // Early return if the key already existed in env.
    if let Ok(value) = env::var(given_key) {
        return Ok(value);
    }

    // Look for the key in .env. We do not set the key in env if it was found.
    for try_kv in dotenvy::dotenv_iter()? {
        let (key, value) = try_kv?;
        if key == given_key {
            return Ok(value);
        }
    }

    Err(dotenvy::Error::EnvVar(env::VarError::NotPresent))
}
