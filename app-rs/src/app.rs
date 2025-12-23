//! The Rust native app state. The interfaces here should look like standard
//! Rust, without any FFI weirdness.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, anyhow, bail};
use common::{
    api::user::{NodePk, NodePkProof, UserPk},
    constants,
    rng::Crng,
    root_seed::RootSeed,
};
use lexe_api::models::command::UpdatePaymentNote;
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::CredentialsRef,
};
use payment_uri::{bip353, lnurl};
use sdk_rust::{
    config::{
        WalletEnv, WalletEnvConfig, WalletEnvDbConfig, WalletUserConfig,
        WalletUserDbConfig,
    },
    ffs::{DiskFs, fsext},
    payments_db::{PaymentSyncSummary, PaymentsDb},
    wallet::{LexeWallet, WithDb},
};
use tracing::{info, instrument, warn};

use crate::{
    app_data::AppDataRs, db::WritebackDb, secret_store::SecretStore,
    settings::SettingsRs,
};

pub struct App {
    wallet: LexeWallet<WithDb>,
    app_db: AppDb,

    wallet_user: WalletUser,
    user_config: WalletUserConfig,
    user_db_config: WalletUserDbConfig,
    use_mock_secret_store: bool,

    /// Whether we've called [`LexeWallet::ensure_provisioned`] yet.
    is_provisioned: AtomicBool,
}

/// App-specific databases.
pub struct AppDb {
    app_data_db: Arc<WritebackDb<AppDataRs>>,
    settings_db: Arc<WritebackDb<SettingsRs>>,
}

/// Basic info about a wallet user.
#[derive(Clone)]
pub struct WalletUser {
    user_pk: UserPk,
    node_pk: NodePk,
    /// Currently, this is only used to support account deletion requests.
    node_pk_proof: NodePkProof,
}

// --- impl App --- //

impl App {
    /// Signup a new user.
    ///
    /// - `backup_password`: set to `Some` if the user is signing up with active
    ///   Google Drive backup. This is the user's backup password, used to
    ///   encrypt their `RootSeed` backup on Google Drive.
    /// - `google_auth_code`: set to `Some` if the user is signing up with
    ///   active Google Drive backup. This is the server auth code passed to the
    ///   node enclave during provisioning.
    #[instrument(skip_all, name = "(signup)")]
    pub async fn signup(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        env_db_config: WalletEnvDbConfig,
        use_mock_secret_store: bool,
        root_seed: &RootSeed,
        partner: Option<UserPk>,
        signup_code: Option<String>,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<Self> {
        let wallet_user = WalletUser::from_seed(rng, root_seed);

        let user_db_config =
            WalletUserDbConfig::new(wallet_user.user_pk, env_db_config.clone());

        // Create fresh wallet
        let credentials = CredentialsRef::from(root_seed);
        let wallet = LexeWallet::fresh(
            rng,
            env_config.clone(),
            credentials,
            env_db_config.lexe_data_dir().clone(),
        )
        .context("Failed to init LexeWallet")?;
        let user_config = wallet.user_config().clone();

        let app_db =
            AppDb::fresh(&user_db_config).context("Failed to init AppDb")?;

        // Signup and provision
        let allow_gvfs_access: bool = true;
        wallet
            .signup_and_provision(
                rng,
                root_seed,
                partner,
                signup_code,
                allow_gvfs_access,
                backup_password,
                google_auth_code,
            )
            .await
            .context("Signup failed")?;

        // We've successfully signed up and provisioned our node; we can finally
        // "commit" and persist our root seed
        let secret_store = SecretStore::new(
            use_mock_secret_store,
            env_config.wallet_env,
            user_db_config.env_db_dir(),
        );
        secret_store
            .write_root_seed(root_seed)
            .context("Failed to persist root seed")?;

        info!(
            user_pk = %wallet_user.user_pk,
            node_pk = %wallet_user.node_pk,
            "New user signed up; node provisioned"
        );

        Ok(Self {
            wallet,
            app_db,
            wallet_user,
            user_config,
            user_db_config,
            use_mock_secret_store,
            is_provisioned: AtomicBool::new(true),
        })
    }

    /// Try to load the root seed from the platform secret store and app state
    /// from the local storage. Returns `None` if this is the first run.
    #[instrument(skip_all, name = "(load)")]
    pub async fn load<R: Crng>(
        rng: &mut R,
        env_config: WalletEnvConfig,
        env_db_config: WalletEnvDbConfig,
        use_mock_secret_store: bool,
    ) -> anyhow::Result<Option<Self>> {
        let secret_store = SecretStore::new(
            use_mock_secret_store,
            env_config.wallet_env,
            env_db_config.env_db_dir(),
        );
        let maybe_root_seed = secret_store
            .read_root_seed()
            .context("Failed to read root seed from SecretStore")?;

        // If there's nothing in the secret store, this must be a fresh install;
        // we can just return here.
        let root_seed = match maybe_root_seed {
            None => return Ok(None),
            Some(s) => s,
        };

        let wallet_user = WalletUser::from_seed(rng, &root_seed);
        let user_db_config =
            WalletUserDbConfig::new(wallet_user.user_pk, env_db_config.clone());

        // Load existing wallet
        let credentials = CredentialsRef::from(&root_seed);
        let wallet = LexeWallet::load(
            rng,
            env_config.clone(),
            credentials,
            env_db_config.lexe_data_dir().clone(),
        )
        .context("Failed to build LexeWallet")?;
        let user_config = wallet.user_config().clone();

        let app_db =
            AppDb::load(&user_db_config).context("Failed to load AppDb")?;

        Ok(Some(Self {
            wallet,
            app_db,
            wallet_user,
            user_config,
            user_db_config,
            use_mock_secret_store,
            is_provisioned: AtomicBool::new(false),
        }))
    }

    /// Restore wallet from backup.
    ///
    /// `google_auth_code`: see [`NodeProvisionRequest::google_auth_code`]
    ///
    /// [`NodeProvisionRequest::google_auth_code`]: common::api::provision::NodeProvisionRequest::google_auth_code
    #[instrument(skip_all, name = "(restore)")]
    pub async fn restore(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        env_db_config: WalletEnvDbConfig,
        use_mock_secret_store: bool,
        google_auth_code: Option<String>,
        root_seed: &RootSeed,
    ) -> anyhow::Result<Self> {
        let wallet_user = WalletUser::from_seed(rng, root_seed);
        let user_db_config =
            WalletUserDbConfig::new(wallet_user.user_pk, env_db_config.clone());

        // Init a fresh LexeWallet
        let credentials = CredentialsRef::from(root_seed);
        let wallet = LexeWallet::fresh(
            rng,
            env_config.clone(),
            credentials,
            env_db_config.lexe_data_dir().clone(),
        )
        .context("Failed to build LexeWallet")?;
        let user_config = wallet.user_config().clone();

        // Potentially restore app db
        let app_db =
            AppDb::load(&user_db_config).context("Failed to load AppDb")?;

        // Ensure we are provisioned to the latest enclaves.
        let allow_gvfs_access = true;
        let credentials = CredentialsRef::from(root_seed);
        let encrypted_seed = None;
        wallet
            .ensure_provisioned(
                credentials,
                allow_gvfs_access,
                encrypted_seed,
                google_auth_code,
            )
            .await
            .context("Re-provision failed")?;
        info!("Successfully re-provisioned to latest releases");

        // We've successfully restored and provisioned our node; we can finally
        // "commit" and persist our root seed
        let secret_store = SecretStore::new(
            use_mock_secret_store,
            env_config.wallet_env,
            user_db_config.env_db_dir(),
        );
        secret_store
            .write_root_seed(root_seed)
            .context("Failed to persist root seed")?;

        info!(
            user_pk = %wallet_user.user_pk, node_pk = %wallet_user.node_pk,
            "Restored user"
        );

        Ok(Self {
            wallet,
            app_db,
            wallet_user,
            user_config,
            user_db_config,
            use_mock_secret_store,
            is_provisioned: AtomicBool::new(true),
        })
    }

    pub async fn provision(&self) -> anyhow::Result<()> {
        let secret_store = SecretStore::new(
            self.use_mock_secret_store,
            self.wallet_env(),
            self.user_db_config.env_db_dir(),
        );
        let maybe_root_seed = secret_store
            .read_root_seed()
            .context("Failed to read root seed from SecretStore")?;

        // If there's nothing in the secret store, this must be a fresh install;
        // we can just return here.
        let root_seed = match maybe_root_seed {
            None => return Err(anyhow!("No root seed found")),
            Some(s) => s,
        };

        let credentials = CredentialsRef::from(&root_seed);
        let allow_gvfs_access = true;
        let encrypted_seed = None;
        let google_auth_code = None;
        self.wallet
            .ensure_provisioned(
                credentials,
                allow_gvfs_access,
                encrypted_seed,
                google_auth_code,
            )
            .await?;
        self.is_provisioned.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Returns the [`NodeClient`] if the app is provisioned.
    pub fn node_client(&self) -> anyhow::Result<&NodeClient> {
        if !self.is_provisioned.load(Ordering::Relaxed) {
            bail!("App is not provisioned");
        }
        Ok(self.wallet.node_client())
    }

    pub fn gateway_client(&self) -> &GatewayClient {
        self.wallet.gateway_client()
    }

    pub fn bip353_client(&self) -> &bip353::Bip353Client {
        self.wallet.bip353_client()
    }

    pub fn lnurl_client(&self) -> &lnurl::LnurlClient {
        self.wallet.lnurl_client()
    }

    fn wallet_env(&self) -> WalletEnv {
        self.user_config.env_config.wallet_env
    }

    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn settings_db(&self) -> Arc<WritebackDb<SettingsRs>> {
        self.app_db.settings_db().clone()
    }

    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn app_data_db(&self) -> Arc<WritebackDb<AppDataRs>> {
        self.app_db.app_data_db().clone()
    }

    // TODO(phlip9): unhack this API when I figure out how to make frb stop auto
    // opaque'ing `AppUserInfo`.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn wallet_user(&self) -> (String, String, String) {
        self.wallet_user.to_ffi()
    }

    #[instrument(skip_all, name = "(sync-payments)")]
    pub async fn sync_payments(&self) -> anyhow::Result<PaymentSyncSummary> {
        let start = Instant::now();
        info!("start");

        let res = self
            .wallet
            .db()
            .sync_payments(
                self.wallet.node_client(),
                constants::DEFAULT_PAYMENTS_BATCH_SIZE,
            )
            .await;

        let elapsed = start.elapsed();
        match &res {
            Ok(summary) => info!("success: elapsed: {elapsed:?}, {summary:?}"),
            Err(err) => warn!("error: elapsed: {elapsed:?}, {err:#?}"),
        }

        res
    }

    pub fn payments_db(&self) -> &PaymentsDb<DiskFs> {
        self.wallet.payments_db()
    }

    pub async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        self.wallet.update_payment_note(req).await
    }
}

// --- impl WalletUser --- //

impl WalletUser {
    /// Derive a [`WalletUser`] from a [`RootSeed`].
    pub fn from_seed(rng: &mut impl Crng, root_seed: &RootSeed) -> Self {
        let user_pk = root_seed.derive_user_pk();
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk = NodePk(node_key_pair.public_key());
        let node_pk_proof = NodePkProof::sign(rng, &node_key_pair);
        Self {
            user_pk,
            node_pk,
            node_pk_proof,
        }
    }

    /// Convert to FFI-friendly strings.
    // NOTE(phlip9): I can't for the life of me figure out why frb keeps trying
    // to RustAutoOpaque wrap the ffi type. Impling the conversion here seems to
    // make it stop???
    pub fn to_ffi(&self) -> (String, String, String) {
        let user_pk = self.user_pk.to_string();
        let node_pk = self.node_pk.to_string();
        let node_pk_proof = self.node_pk_proof.to_hex_string();
        (user_pk, node_pk, node_pk_proof)
    }
}

// --- impl AppDb --- //

impl AppDb {
    /// Create fresh databases, deleting any existing data.
    pub fn fresh(user_db_config: &WalletUserDbConfig) -> anyhow::Result<Self> {
        let settings_db_dir = Self::settings_db_dir(user_db_config);
        let settings_ffs = DiskFs::create_clean_dir_all(settings_db_dir)
            .context("Could not create settings ffs")?;
        let settings_db = Arc::new(SettingsRs::load(settings_ffs));

        let app_data_db_dir = Self::app_data_db_dir(user_db_config);
        let app_data_ffs = DiskFs::create_clean_dir_all(app_data_db_dir)
            .context("Could not create app data ffs")?;
        let app_data_db = Arc::new(AppDataRs::load(app_data_ffs));

        // Delete the old app_data_db dir in case it exists.
        let old_dir = Self::old_app_data_db_dir(user_db_config);
        match fsext::remove_dir_all_idempotent(&old_dir) {
            Ok(()) => info!("Deleted old app_data_db dir: {old_dir:?}"),
            Err(e) => warn!(?old_dir, "Couldn't delete old dir: {e:#}"),
        }

        Ok(Self {
            app_data_db,
            settings_db,
        })
    }

    /// Load existing databases (or create new ones if none exist).
    pub fn load(user_db_config: &WalletUserDbConfig) -> anyhow::Result<Self> {
        let settings_db_dir = Self::settings_db_dir(user_db_config);
        let settings_ffs = DiskFs::create_dir_all(settings_db_dir)
            .context("Could not create settings ffs")?;
        let settings_db = Arc::new(SettingsRs::load(settings_ffs));

        let app_data_db_dir = Self::app_data_db_dir(user_db_config);
        let app_data_ffs = DiskFs::create_dir_all(app_data_db_dir)
            .context("Could not create app data ffs")?;
        let app_data_db = Arc::new(AppDataRs::load(app_data_ffs));

        // If nothing was read, it's possible the user just upgraded to the
        // latest path. Delete the old dir just in case.
        if app_data_db.read() == AppDataRs::default() {
            let old_dir = Self::old_app_data_db_dir(user_db_config);
            match fsext::remove_dir_all_idempotent(&old_dir) {
                Ok(()) => info!("Deleted old app_data_db dir: {old_dir:?}"),
                Err(e) => warn!(?old_dir, "Couldn't delete old dir: {e:#}"),
            }
        }

        Ok(Self {
            app_data_db,
            settings_db,
        })
    }

    pub(crate) fn app_data_db(&self) -> &Arc<WritebackDb<AppDataRs>> {
        &self.app_data_db
    }

    pub(crate) fn settings_db(&self) -> &Arc<WritebackDb<SettingsRs>> {
        &self.settings_db
    }

    fn app_data_db_dir(user_db_config: &WalletUserDbConfig) -> PathBuf {
        user_db_config.user_db_dir().join("app_data_db")
    }

    fn old_app_data_db_dir(user_db_config: &WalletUserDbConfig) -> PathBuf {
        user_db_config.user_db_dir().join("app_db")
    }

    fn settings_db_dir(user_db_config: &WalletUserDbConfig) -> PathBuf {
        user_db_config.user_db_dir().join("settings_db")
    }
}
