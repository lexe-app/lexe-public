//! The `SecretStore` persists user secrets like the [`RootSeed`] in each
//! platform's standard secrets keychain.
//!
//! Uses [`hwchen/keychain-rs`](https://github.com/hwchen/keyring-rs) for all
//! platforms except Android.
//!
//! * **Linux:** uses the desktop secret-service via dbus.
//! * **macOS+iOS:** uses Keychain
//! * **Windows:** uses wincreds
//! * **Android:** stores it in a file in the app data directory (accessing the
//!   JVM-only [`Android Keystore`](https://developer.android.com/training/articles/keystore)
//!   is a huge pain). Fortunately, this isn't too awful, since app data is
//!   sandboxed and inaccessible to other apps.
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

use crate::app::{AppConfig, BuildFlavor};

pub struct SecretStore {
    root_seed_entry: Mutex<keyring::Entry>,
}

impl SecretStore {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    fn service_name(build: BuildFlavor) -> String {
        format!("app.lexe.lexeapp.{build}")
    }

    /// Create a new `SecretStore`.
    ///
    /// For all platforms except Android, this will use the user's OS-provided
    /// keyring. Android will just store secrets in the app's internal data
    /// directory. See module comments for more details.
    pub fn new(config: &AppConfig) -> Self {
        if config.use_mock_secret_store {
            // Some tests rely on a persistent (tempdir) mock secret store
            return Self::file(&config.app_data_dir);
        }

        cfg_if! {
            if #[cfg(not(target_os = "android"))] {
                Self::keyring(config.build_flavor())
            } else {
                Self::file(&config.app_data_dir)
            }
        }
    }

    /// A secret store that uses the system keychain.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    fn keyring(build: BuildFlavor) -> Self {
        let service = Self::service_name(build);
        Self::keyring_inner(&service)
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    fn keyring_inner(service: &str) -> Self {
        let entry = keyring::Entry::new(service, "root_seed.hex").unwrap();
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    /// A secret store that just dumps secrets into the app-specific data
    /// directory. Currently only used on Android.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn file(app_data_dir: &Path) -> Self {
        let credential =
            Box::new(FileCredential::new(app_data_dir.join("root_seed.hex")));
        let entry = keyring::Entry::new_with_credential(credential);
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    /// Create a mock SecretStore. Writing to this mock store does not actually
    /// persist them.
    #[allow(unused)] // Not used, but leaving around in case it is useful later
    fn mock() -> Self {
        let mock = keyring::mock::MockCredentialBuilder {}
            .build(None, "mock", "root_seed.hex")
            .unwrap();
        let entry = keyring::Entry::new_with_credential(mock);
        Self {
            root_seed_entry: Mutex::new(entry),
        }
    }

    /// Delete all stored secrets
    pub fn delete(&self) -> anyhow::Result<()> {
        self.delete_root_seed()
            .context("Failed to delete SecretStore")
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
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
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
    use common::rng::SysRng;

    use super::*;

    fn test_secret_store(rng: &mut SysRng, secret_store: &SecretStore) {
        assert!(secret_store.read_root_seed().unwrap().is_none());

        let root_seed = RootSeed::from_rng(rng);
        secret_store.write_root_seed(&root_seed).unwrap();

        let root_seed2 = secret_store.read_root_seed().unwrap().unwrap();
        assert_eq!(root_seed.expose_secret(), root_seed2.expose_secret());

        secret_store.delete_root_seed().unwrap();
        assert!(secret_store.read_root_seed().unwrap().is_none());
    }

    // ignore android: android only supports file_store
    // ignore linux: keyring_store only works with GUI and not headless,
    // e.g. our dev server
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    #[test]
    fn test_keyring_store() {
        use std::ffi::OsStr;

        use common::rng::RngExt;

        // SKIP this test in CI, since the Github CI instance is headless and/or
        // doesn't give us access to the gnome keyring.
        if std::env::var_os("LEXE_CI").as_deref() == Some(OsStr::new("1")) {
            return;
        }

        let mut rng = SysRng::new();
        let dummy_id = rng.gen_u64();

        // use a dummy service name to be absolutely sure we don't clobber any
        // existing keyring entry.
        let dummy_service = format!("lexe.dummy.{:08x}", dummy_id);
        let secret_store = SecretStore::keyring_inner(&dummy_service);
        test_secret_store(&mut rng, &secret_store);
    }

    #[test]
    fn test_file_store() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut rng = SysRng::new();

        let secret_store = SecretStore::file(tempdir.path());
        test_secret_store(&mut rng, &secret_store);
    }
}
