use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use lexe::{
    config::WalletEnvConfig,
    types::auth::{ClientCredentials, Credentials, CredentialsRef},
    wallet::LexeWallet,
};
use lexe_crypto::ed25519;
use quick_cache::unsync;

/// Max number of per-request [`LexeWallet`]s kept in memory.
const WALLET_CACHE_CAPACITY: usize = 64;

/// The [`LexeWallet`]s served by the sidecar: the default wallet, plus one per
/// set of per-request [`ClientCredentials`].
pub(crate) struct Wallets {
    /// Per-request wallets, keyed by `client_pk`.
    cache: Mutex<unsync::Cache<ed25519::PublicKey, Arc<LexeWallet>>>,
    data_dir: PathBuf,
    /// The default [`LexeWallet`] and its [`Credentials`] from env/CLI.
    default: Option<(Arc<LexeWallet>, Arc<Credentials>)>,
    env_config: WalletEnvConfig,
}

impl Wallets {
    pub(crate) fn new(
        data_dir: PathBuf,
        default: Option<(Arc<LexeWallet>, Arc<Credentials>)>,
        env_config: WalletEnvConfig,
    ) -> Self {
        Self {
            cache: Mutex::new(unsync::Cache::new(WALLET_CACHE_CAPACITY)),
            data_dir,
            default,
            env_config,
        }
    }

    /// The default wallet and its credentials, if configured.
    pub(crate) fn default(
        &self,
    ) -> Option<&(Arc<LexeWallet>, Arc<Credentials>)> {
        self.default.as_ref()
    }

    /// Get the wallet for these per-request client credentials.
    pub(crate) fn get(
        &self,
        credentials: &ClientCredentials,
    ) -> anyhow::Result<Arc<LexeWallet>> {
        let client_pk = credentials.unstable().client_pk;
        let mut locked_cache = self.cache.lock().unwrap();
        if let Some(wallet) = locked_cache.get(&client_pk) {
            return Ok(wallet.clone());
        }

        let wallet = LexeWallet::load_or_fresh(
            self.env_config.clone(),
            CredentialsRef::from(credentials),
            Some(self.data_dir.clone()),
        )
        .map(Arc::new)
        .context("Failed to create wallet")?;
        locked_cache.insert(client_pk, wallet.clone());
        Ok(wallet)
    }
}
