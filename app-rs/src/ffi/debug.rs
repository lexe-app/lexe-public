//! Debug methods for use during development.

use crate::{ffi::types::Config, secret_store::SecretStore};

/// Delete the local persisted `SecretStore` and `RootSeed`.
///
/// WARNING: you will need a backup recovery to use the account afterwards.
///
/// flutter_rust_bridge:sync
pub fn delete_secret_store(config: Config) -> anyhow::Result<()> {
    config.validate();

    SecretStore::new(
        config.use_mock_secret_store,
        config.wallet_env(),
        config.env_db_config().env_db_dir(),
    )
    .delete()
}

/// Delete the local latest_release file.
///
/// flutter_rust_bridge:sync
pub fn delete_latest_provisioned(config: Config) -> anyhow::Result<()> {
    config.validate();

    // TODO(phlip9): re-impl. will need to take `AppHandle`.
    // let app_config = AppConfig::from(config);
    // let app_data_ffs = FlatFileFs::create_dir_all(app_config.env_data_dir)
    //     .context("Could not create app data ffs")?;
    // storage::delete_latest_provisioned(&app_data_ffs)?;
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
