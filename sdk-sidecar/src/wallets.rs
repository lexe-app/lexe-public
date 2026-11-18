use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, ensure};
use lexe::{
    config::WalletEnvConfig,
    types::auth::{ClientCredentials, Credentials, CredentialsRef, UserPk},
    wallet::LexeWallet,
};
use lexe_api::revocable_clients::scopes::Permission;
use lexe_crypto::ed25519;
use quick_cache::unsync;

/// Max number of per-request [`LexeWallet`]s kept in memory.
const WALLET_CACHE_CAPACITY: usize = 64;

/// The [`LexeWallet`]s served by the sidecar: the default wallet, plus one per
/// set of per-request [`ClientCredentials`].
///
/// Client credentials' `user_pk` is self-reported, so before a per-request
/// wallet may touch the local data dir, the user's node must confirm it.
pub(crate) struct Wallets {
    /// Per-request wallets, keyed by `client_pk`.
    cache: Mutex<unsync::Cache<ed25519::PublicKey, Arc<LexeWallet>>>,
    data_dir: PathBuf,
    /// The default [`LexeWallet`] and its [`Credentials`] from env/CLI.
    default: Option<(Arc<LexeWallet>, Arc<Credentials>)>,
    env_config: WalletEnvConfig,
    /// Per-request credentials' [`Verification`]s, keyed by `client_pk`.
    /// Verifying wakes the user's node, so these are never evicted.
    verifications: Mutex<HashMap<ed25519::PublicKey, Verification>>,
}

/// What the user's node reported about some client credentials.
#[derive(Copy, Clone)]
struct Verification {
    /// The user whose node accepted the credentials.
    user_pk: UserPk,
    /// Whether the client may read all of the user's payments.
    can_read_payments: bool,
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
            verifications: Mutex::new(HashMap::new()),
        }
    }

    /// The default wallet and its credentials, if configured.
    pub(crate) fn default(
        &self,
    ) -> Option<&(Arc<LexeWallet>, Arc<Credentials>)> {
        self.default.as_ref()
    }

    /// Get the wallet for these per-request client credentials.
    ///
    /// The wallet persists to the shared data dir only if the client may read
    /// the user's payments, which the local payments cache would reveal.
    /// Otherwise, it's served without local persistence.
    pub(crate) async fn get(
        &self,
        credentials: &ClientCredentials,
    ) -> anyhow::Result<Arc<LexeWallet>> {
        let verification = self.verify(credentials).await?;
        ensure!(
            credentials.unstable().user_pk.map(UserPk::from_unstable)
                == Some(verification.user_pk),
            "Client credentials' user_pk doesn't match their node's",
        );
        let persist = verification.can_read_payments;

        let client_pk = credentials.unstable().client_pk;
        let mut locked_cache = self.cache.lock().unwrap();
        if let Some(wallet) = locked_cache.get(&client_pk) {
            return Ok(wallet.clone());
        }

        let env_config = self.env_config.clone();
        let credentials_ref = CredentialsRef::from(credentials);
        let wallet = if persist {
            let data_dir = Some(self.data_dir.clone());
            LexeWallet::load_or_fresh(env_config, credentials_ref, data_dir)
        } else {
            LexeWallet::without_db(env_config, credentials_ref)
        }
        .map(Arc::new)
        .context("Failed to create wallet")?;
        locked_cache.insert(client_pk, wallet.clone());
        Ok(wallet)
    }

    /// Get the [`Verification`] for these credentials, asking the user's node
    /// only if they haven't been verified before.
    async fn verify(
        &self,
        credentials: &ClientCredentials,
    ) -> anyhow::Result<Verification> {
        let client_pk = credentials.unstable().client_pk;
        if let Some(v) = self.verifications.lock().unwrap().get(&client_pk) {
            return Ok(*v);
        }

        // Concurrent first requests may each ask the node, which is harmless.
        let verification =
            helpers::request_verification(&self.env_config, credentials)
                .await?;
        self.verifications
            .lock()
            .unwrap()
            .insert(client_pk, verification);
        Ok(verification)
    }
}

mod helpers {
    use super::*;

    /// Asks the user's node about these credentials. Wakes the node.
    pub(super) async fn request_verification(
        env_config: &WalletEnvConfig,
        credentials: &ClientCredentials,
    ) -> anyhow::Result<Verification> {
        let wallet = LexeWallet::without_db(
            env_config.clone(),
            CredentialsRef::from(credentials),
        )?;
        let info = wallet
            .client_info()
            .await
            .context("Failed to verify client credentials with their node")?;

        let has_permission = |permission: Permission| {
            let id = permission.as_str();
            info.effective_permissions.iter().any(|p| p == id)
        };

        // Nodes too old to report `user_pk` in `client_info` still report it
        // in `node_info`.
        // compat: Remove once all nodes are node-v0.10.10+
        let user_pk = match info.user_pk {
            Some(user_pk) => user_pk,
            None => {
                ensure!(
                    has_permission(Permission::NodeInfo),
                    "Can't verify client credentials without the `read_info` \
                     scope until the user's node is upgraded",
                );
                wallet
                    .node_info()
                    .await
                    .context(
                        "Failed to verify client credentials with their node",
                    )?
                    .user_pk
            }
        };

        Ok(Verification {
            user_pk,
            can_read_payments: has_permission(Permission::GetUpdatedPayments),
        })
    }
}
