//! The `SecretStore` persists user secrets like the [`RootSeed`] in each
//! platform's standard secrets keychain.
//!
//! Uses [`hwchen/keychain-rs`](https://github.com/hwchen/keyring-rs) for all
//! platforms except Android.
//!
//! * **Linux:** uses the desktop secret-service via dbus.
//! * **macOS+iOS:** uses Keychain
//! * Windows: uses wincreds
//! * Android: stores it in a file in the app data directory (accessing the
//!   JVM-only [`Android Keystore`](https://developer.android.com/training/articles/keystore)
//!   is a huge pain)
//!
//! [`RootSeed`]: common::root_seed::RootSeed

use std::{
    io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Mutex,
};

use anyhow::Context;
use cfg_if::cfg_if;
use common::{hex, root_seed::RootSeed};
use keyring::credential::{CredentialApi, CredentialBuilderApi};
use secrecy::ExposeSecret;

use crate::{app::AppConfig, bindings::DeployEnv};

pub struct SecretStore {
    root_seed_entry: Mutex<keyring::Entry>,
}

impl SecretStore {
    #[allow(dead_code)]
    fn service_name(deploy_env: DeployEnv) -> String {
        let env = deploy_env.as_str();
        format!("tech.lexe.lexeapp.{env}")
    }

    /// Create a new `SecretStore`.
    ///
    /// For all platforms except Android, this will use the user's OS-provided
    /// keyring. Android will just store secrets in the app's internal data
    /// directory, since .
    pub fn new(config: &AppConfig) -> Self {
        if config.use_mock_secret_store {
            return Self::mock();
        }

        cfg_if! {
            if #[cfg(not(target_os = "android"))] {
                Self::keyring(config.deploy_env)
            } else {
                Self::file(config.deploy_env, &config.app_data_dir)
            }
        }
    }

    /// A secret store that uses the system keychain.
    #[allow(dead_code)]
    fn keyring(deploy_env: DeployEnv) -> Self {
        let service = Self::service_name(deploy_env);
        Self::keyring_inner(&service)
    }

    #[allow(dead_code)]
    fn keyring_inner(service: &str) -> Self {
        let entry = keyring::Entry::new(service, "rootseed").unwrap();
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    /// A secret store that just dumps secrets into the app-specific data
    /// directory. Currently only used on Android.
    #[allow(dead_code)]
    fn file(deploy_env: DeployEnv, app_data_dir: &Path) -> Self {
        let env = deploy_env.as_str();
        let credential = Box::new(FileCredential {
            path: app_data_dir.join(format!("{env}.rootseed")),
        });
        let entry = keyring::Entry::new_with_credential(credential);
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    /// Create a mock SecretStore. Writing to this mock store does not actually
    /// persist them.
    fn mock() -> Self {
        let mock = keyring::mock::MockCredentialBuilder {}
            .build(None, "mock", "rootseed")
            .unwrap();
        let entry = keyring::Entry::new_with_credential(mock);
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    pub fn read_root_seed(&self) -> anyhow::Result<Option<RootSeed>> {
        let res = self.root_seed_entry.lock().unwrap().get_password();
        match res {
            Ok(s) => {
                let root_seed = RootSeed::from_str(&s).context(
                    "Found the root seed, but it's not the right size",
                )?;
                Ok(Some(root_seed))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(anyhow::Error::new(err)
                .context("Failed to read root seed from keyring")),
        }
    }
    pub fn write_root_seed(&self, root_seed: &RootSeed) -> anyhow::Result<()> {
        let root_seed_hex = hex::encode(root_seed.expose_secret().as_slice());
        self.root_seed_entry
            .lock()
            .unwrap()
            .set_password(&root_seed_hex)
            .context("Failed to write root seed into keyring")
    }
    pub fn delete_root_seed(&self) -> anyhow::Result<()> {
        self.root_seed_entry
            .lock()
            .unwrap()
            .delete_password()
            .context("Failed to delete root seed from keyring")
    }
}

/// A small shim that dumps a credential (e.g., the `RootSeed`) into a file.
struct FileCredential {
    path: PathBuf,
}

impl FileCredential {
    #[allow(dead_code)]
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

fn io_err_to_keyring_err(err: io::Error) -> keyring::Error {
    match err.kind() {
        io::ErrorKind::NotFound => keyring::Error::NoEntry,
        io::ErrorKind::PermissionDenied =>
            keyring::Error::NoStorageAccess(err.into()),
        _ => keyring::Error::PlatformFailure(err.into()),
    }
}

impl CredentialApi for FileCredential {
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err_to_keyring_err)?;
        }

        std::fs::write(&self.path, password.as_bytes())
            .map_err(io_err_to_keyring_err)
    }
    fn get_password(&self) -> keyring::Result<String> {
        let bytes = std::fs::read(&self.path).map_err(io_err_to_keyring_err)?;
        String::from_utf8(bytes)
            .map_err(|err| keyring::Error::BadEncoding(err.into_bytes()))
    }
    fn delete_password(&self) -> keyring::Result<()> {
        std::fs::remove_file(&self.path).map_err(io_err_to_keyring_err)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod test {
    use common::rng::{RngCore, SysRng};

    use super::*;
    use crate::bindings::DeployEnv;

    fn test_secret_store(rng: &mut SysRng, secret_store: &SecretStore) {
        assert!(secret_store.read_root_seed().unwrap().is_none());

        let root_seed = RootSeed::from_rng(rng);
        secret_store.write_root_seed(&root_seed).unwrap();

        let root_seed2 = secret_store.read_root_seed().unwrap().unwrap();
        assert_eq!(root_seed.expose_secret(), root_seed2.expose_secret());

        secret_store.delete_root_seed().unwrap();
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn test_keyring_store() {
        let mut rng = SysRng::new();
        let mut buf = [0u8; 8];
        rng.fill_bytes(&mut buf);

        // use a dummy service name to be absolutely sure we don't clobber any
        // existing keyring entry.
        let dummy_service =
            format!("lexe.dummy.{:08x}", u64::from_le_bytes(buf));
        let secret_store = SecretStore::keyring_inner(&dummy_service);
        test_secret_store(&mut rng, &secret_store);
    }

    #[test]
    fn test_file_store() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut rng = SysRng::new();

        let secret_store = SecretStore::file(DeployEnv::Dev, tempdir.path());
        test_secret_store(&mut rng, &secret_store);
    }
}
