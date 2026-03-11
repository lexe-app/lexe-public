//! Use `OrEnvExt` when you have a cli arg (e.g. `db_url: Option<String>`) that
//! can also be set by a "fallback" env variable (e.g. `$DATABASE_URL`). Then,
//! when initializing the args, just use `db_url.or_env_mut("DATABASE_URL")?`.
//!
//! This works with any `FromStr` type, in which case it will parse the env
//! value and return an error if that fails.

use std::{env, str::FromStr};

use anyhow::Context;

pub trait OrEnvExt: Sized {
    /// Analogous to [`Option::or_else`]. Takes ownership of the arg if set,
    /// otherwise initializes the arg from env. Used to initialize args lazily.
    /// Also has good ergonomics with [`Result`] / [`Option`] chains.
    fn or_env(mut self, env_var: &'static str) -> anyhow::Result<Self> {
        self.or_env_mut(env_var)?;
        Ok(self)
    }

    /// If the arg is not set, initialize the arg from env by mutating the arg
    /// in place. Used to proactively initialize and validate args.
    fn or_env_mut(
        &mut self,
        env_var: &'static str,
    ) -> anyhow::Result<&mut Self>;
}

fn env_var_opt(env_var: &'static str) -> anyhow::Result<Option<String>> {
    match env::var(env_var) {
        Ok(val_str) => Ok(Some(val_str)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(s)) =>
            Err(anyhow::format_err!("invalid unicode: '{:?}'", s)),
    }
}

impl<T> OrEnvExt for Option<T>
where
    T: FromStr,
    T::Err: Into<anyhow::Error>,
{
    fn or_env_mut(
        &mut self,
        env_var: &'static str,
    ) -> anyhow::Result<&mut Option<T>> {
        if self.is_none() {
            // If no env var, do nothing. Error if not UTF-8 encoded.
            let val_str = match env_var_opt(env_var).context(env_var)? {
                Some(v) => v,
                None => return Ok(self),
            };
            let val = T::from_str(&val_str)
                .map_err(Into::into)
                .with_context(|| format!("Invalid env value `${env_var}`"))?;
            *self = Some(val);
        }

        Ok(self)
    }
}

impl OrEnvExt for bool {
    fn or_env_mut(
        &mut self,
        env_var: &'static str,
    ) -> anyhow::Result<&mut bool> {
        if !*self {
            // If no env var, do nothing. Error if not UTF-8 encoded.
            let val_str = match env_var_opt(env_var).context(env_var)? {
                Some(v) => v,
                None => return Ok(self),
            };
            let val = bool::from_str(&val_str)
                .with_context(|| format!("Invalid env value `${env_var}`"))?;
            *self = val;
        }

        Ok(self)
    }
}
