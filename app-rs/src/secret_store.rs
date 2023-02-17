//! The `SecretsStore` persists user secrets like the [`RootSeed`] in each
//! platform's standard secrets keychain or secure enclave.
//!
//! [`RootSeed`]: common::root_seed::RootSeed

use common::root_seed::RootSeed;

#[derive(Default)]
pub struct SecretStore;

impl SecretStore {
    pub fn new() -> Self {
        Self
    }

    pub fn read_root_seed(&self) -> anyhow::Result<Option<RootSeed>> {
        Ok(None)
    }
    pub fn write_root_seed(&self, _root_seed: &RootSeed) -> anyhow::Result<()> {
        Ok(())
    }
}

// pub struct MockSecretsStore;
//
// impl MockSecretsStore {
//     pub fn new() -> Self {
//         Self
//     }
// }
//
// impl SecretsStore for MockSecretsStore {
//     fn read_root_seed(&self) -> anyhow::Result<Option<RootSeed>> {
//         Ok(None)
//     }
//     fn write_root_seed(&self, _root_seed: &RootSeed) -> anyhow::Result<()> {
//         Ok(())
//     }
// }
