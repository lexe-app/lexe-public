use flutter_rust_bridge::RustOpaqueNom;

use crate::ffi::types::{Config, RootSeed};
pub(crate) use crate::secret_store::SecretStore as SecretStoreRs;

/// Dart interface to the app secret store.
pub struct SecretStore {
    pub inner: RustOpaqueNom<SecretStoreRs>,
}

impl SecretStore {
    /// Create a handle to the secret store for the current app configuration.
    ///
    /// flutter_rust_bridge:sync
    pub fn new(config: Config) -> Self {
        config.validate();

        let inner = RustOpaqueNom::new(SecretStoreRs::new(
            config.use_mock_secret_store,
            config.wallet_env(),
            config.env_db_config().env_db_dir(),
        ));

        Self { inner }
    }

    /// Read the user's root seed from the secret store.
    ///
    /// flutter_rust_bridge:sync
    pub fn read_root_seed(&self) -> anyhow::Result<Option<RootSeed>> {
        let maybe_root_seed = self.inner.read_root_seed()?;

        Ok(maybe_root_seed.map(RootSeed::from))
    }
}
