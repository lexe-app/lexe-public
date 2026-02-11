//! The `SecretStore` persists user secrets like the [`RootSeed`] in each
//! platform's standard secrets keychain.
//!
//! Uses [`hwchen/keychain-rs`](https://github.com/hwchen/keyring-rs) for all
//! platforms except Android.
//!
//! * **Linux:** uses the desktop secret-service via dbus. If you're on
//!   Ubuntu/Pop!_OS/some Gnome distro, then you can inspect the secrets with
//!   `seahorse`.
//! * **macOS+iOS:** uses Keychain.app
//! * **Windows:** uses wincreds
//! * **Android:** stores it in a file in the app data directory (accessing the
//!   JVM-only [`Android Keystore`](https://developer.android.com/training/articles/keystore)
//!   is a huge pain). Fortunately, this isn't too awful, since app data is
//!   sandboxed and inaccessible to other apps.
//!   In the future, consider using something like:
//!   <https://github.com/animo/secure-env/blob/main/src/android.rs> or
//!   <https://gitlab.com/veilid/keyring-manager/-/blob/master/src/android.rs>
//!
//! [`RootSeed`]: common::root_seed::RootSeed

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    thread,
};

use anyhow::Context;
use cfg_if::cfg_if;
use common::root_seed::RootSeed;
use keyring::credential::{CredentialApi, CredentialBuilderApi};
use sdk_rust::config::WalletEnv;
use secrecy::ExposeSecret;

/// Persists user secrets like the [`RootSeed`] in each platform's standard
/// secrets keychain. See module-level docs for platform-specific details.
// TODO(phlip9): support "multi-wallet", i.e., storing multiple root seeds for
// the same target env. Would need UI to switch between wallets, a setting
// for "preferred/default wallet", and graceful shutdown implemented.
pub struct SecretStore {
    root_seed_cred: Box<dyn CredentialApi + Send + Sync>,
}

impl SecretStore {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    fn service_name(wallet_env: WalletEnv) -> String {
        format!("app.lexe.lexeapp.{wallet_env}")
    }

    /// Create a new `SecretStore`.
    ///
    /// For all platforms except Android, this will use the user's OS-provided
    /// keychain. Android will just store secrets in the wallet env database
    /// directory. See module comments for more details.
    pub fn new(
        use_mock_secret_store: bool,
        wallet_env: WalletEnv,
        env_db_dir: &Path,
    ) -> Self {
        if use_mock_secret_store {
            // Some tests rely on a persistent (tempdir) mock secret store
            return Self::file(env_db_dir);
        }

        cfg_if! {
            if #[cfg(target_os = "android")] {
                let _ = wallet_env;
                Self::file(env_db_dir)
            } else {
                Self::keychain(wallet_env)
            }
        }
    }

    /// A secret store that uses the system keychain.
    #[cfg(not(target_os = "android"))]
    fn keychain(wallet_env: WalletEnv) -> Self {
        let service = Self::service_name(wallet_env);

        Self::keychain_inner(&service)
    }

    #[cfg(not(target_os = "android"))]
    fn keychain_inner(service: &str) -> Self {
        let target = None;
        let user = "root_seed.hex";

        cfg_if! {
            if #[cfg(target_os = "ios")] {
                use keyring::ios::IosCredential;
                let cred =
                    IosCredential::new_with_target(target, service, user);
                Self { root_seed_cred: Box::new(cred.unwrap()) }
            } else if #[cfg(target_os = "macos")] {
                use keyring::macos::MacCredential;
                let cred =
                    MacCredential::new_with_target(target, service, user);
                Self { root_seed_cred: Box::new(cred.unwrap()) }
            } else if #[cfg(target_os = "linux")] {
                use keyring::secret_service::SsCredential;
                let cred =
                    SsCredential::new_with_target(target, service, user);
                let cred = ThreadKeyringCredential(Box::new(cred.unwrap()));
                Self { root_seed_cred: Box::new(cred) }
            } else {
                compile_error!("Configure a keychain backend for this OS")
            }
        }
    }

    /// A secret store that just dumps secrets into the wallet env database
    /// directory. Currently only used on Android.
    // TODO(max): Support multi-user secret store
    fn file(env_db_dir: &Path) -> Self {
        Self {
            root_seed_cred: Box::new(FileCredential::new(
                env_db_dir.join("root_seed.hex"),
            )),
        }
    }

    /// Create a mock SecretStore. Writing to this mock store does not actually
    /// persist them.
    #[allow(unused)] // Not used, but leaving around in case it is useful later
    fn mock() -> Self {
        Self {
            root_seed_cred: keyring::mock::MockCredentialBuilder {}
                .build(None, "mock", "root_seed.hex")
                .unwrap(),
        }
    }

    /// Delete all stored secrets.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub fn delete(&self) -> anyhow::Result<()> {
        self.delete_root_seed()
            .context("Failed to delete SecretStore")
    }

    /// Read the user's root seed from the secret store.
    pub fn read_root_seed(&self) -> anyhow::Result<Option<RootSeed>> {
        let res = self.root_seed_cred.get_password();
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

    /// Write the user's root seed to the secret store.
    pub fn write_root_seed(&self, root_seed: &RootSeed) -> anyhow::Result<()> {
        let root_seed_hex = hex::encode(root_seed.expose_secret().as_slice());
        self.root_seed_cred
            .set_password(&root_seed_hex)
            .context("Failed to write root seed into keyring")
    }

    /// Delete the user's root seed from the secret store.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub fn delete_root_seed(&self) -> anyhow::Result<()> {
        self.root_seed_cred
            .delete_password()
            .context("Failed to delete root seed from keyring")
    }
}

/// A small shim that dumps a credential (e.g., the `RootSeed`) into a file.
struct FileCredential {
    path: PathBuf,
}

impl FileCredential {
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

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);

        // Set the file permissions to rw------- (owner r/w only)
        #[cfg(unix)]
        opts.mode(0o600);

        opts.open(self.path.as_path())
            .and_then(|mut file| file.write_all(password.as_bytes()))
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

/// A small shim around a [`keyring::Credential`] that does each operation
/// inside a newly spawned thread.
///
/// This exists just to support Linux, whose `keyring::secret_store` impl uses
/// a tokio `block_on` somewhere inside. Since we normally call the
/// `SecretStore` from async code, this will panic without this. Running all
/// keyring ops from inside their own temporary thread solves the issue.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct ThreadKeyringCredential(Box<dyn CredentialApi + Send + Sync>);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl ThreadKeyringCredential {
    fn thread_op<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
        F: Send,
        R: Send,
    {
        thread::scope(|s| s.spawn(f).join().expect("Thread panicked"))
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl CredentialApi for ThreadKeyringCredential {
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        Self::thread_op(|| self.0.set_password(password))
    }

    fn get_password(&self) -> keyring::Result<String> {
        Self::thread_op(|| self.0.get_password())
    }

    fn delete_password(&self) -> keyring::Result<()> {
        Self::thread_op(|| self.0.delete_password())
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

        // SKIP this test in CI, since the Github CI instance is headless and/or
        // doesn't give us access to the gnome keyring.
        if std::env::var_os("LEXE_CI").as_deref() == Some(OsStr::new("1")) {
            return;
        }

        test_keyring_secret_store_inner();
    }

    // `cargo test -p app-rs -- test_keyring_store_linux --ignored`
    //
    // NOTE: don't remove this async. The linux keyring-rs backend does a
    // `block_on` "under-the-hood" and running this test in an async block
    // ensures we can call it like we normally do (that is, inside an outer
    // `block_on`).
    #[cfg(not(target_os = "android"))]
    #[tokio::test]
    #[ignore]
    async fn test_keyring_store_linux() {
        test_keyring_secret_store_inner();
    }

    #[cfg(not(target_os = "android"))]
    fn test_keyring_secret_store_inner() {
        use common::rng::RngExt;

        let mut rng = SysRng::new();
        let dummy_id = rng.gen_u64();

        // use a dummy service name to be absolutely sure we don't clobber any
        // existing keyring entry.
        let dummy_service = format!("lexe.dummy.{:08x}", dummy_id);
        let secret_store = SecretStore::keychain_inner(&dummy_service);
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
