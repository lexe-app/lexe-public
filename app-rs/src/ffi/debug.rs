//! Debug methods for use during development.

use anyhow::Context;
use flutter_rust_bridge::frb;

use crate::{
    app::AppConfig, ffi::types::Config, ffs::FlatFileFs,
    secret_store::SecretStore, storage,
};

/// Delete the local persisted `SecretStore` and `RootSeed`.
///
/// WARNING: you will need a backup recovery to use the account afterwards.
#[frb(sync)]
pub fn delete_secret_store(config: Config) -> anyhow::Result<()> {
    SecretStore::new(&config.into()).delete()
}

/// Delete the local latest_release file.
#[frb(sync)]
pub fn delete_latest_provisioned(config: Config) -> anyhow::Result<()> {
    let app_config = AppConfig::from(config);
    let app_data_ffs = FlatFileFs::create_dir_all(app_config.app_data_dir)
        .context("Could not create app data ffs")?;
    storage::delete_latest_provisioned(&app_data_ffs)?;
    Ok(())
}

/// Unconditionally panic (for testing).
pub fn unconditional_panic() {
    panic!("Panic inside app-rs");
}

/// Unconditionally return Err (for testing).
pub fn unconditional_error() -> anyhow::Result<()> {
    Err(anyhow::format_err!("Error inside app-rs"))
}
