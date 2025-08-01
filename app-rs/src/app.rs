//! The Rust native app state. The interfaces here should look like standard
//! Rust, without any FFI weirdness.

use std::{
    collections::BTreeSet,
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{anyhow, Context};
use bitcoin::secp256k1;
use common::{
    api::{
        auth::{
            UserSignupRequestWire, UserSignupRequestWireV1,
            UserSignupRequestWireV2,
        },
        provision::NodeProvisionRequest,
        user::{NodePk, NodePkProof, UserPk},
        version::NodeRelease,
    },
    constants,
    env::DeployEnv,
    ln::network::LxNetwork,
    rng::Crng,
    root_seed::RootSeed,
    Secret,
};
use lexe_api::def::{AppBackendApi, AppGatewayApi, AppNodeProvisionApi};
use lexe_std::Apply;
use lexe_tokio::task::LxTask;
use secrecy::ExposeSecret;
use tracing::{info, info_span, instrument, warn};

use crate::{
    client::{Credentials, GatewayClient, NodeClient},
    ffs::{Ffs, FlatFileFs},
    payments::{self, PaymentDb, PaymentSyncSummary},
    provision_history::ProvisionHistory,
    secret_store::SecretStore,
    settings::SettingsDb,
    types::GDriveSignupCredentials,
};

pub struct App {
    gateway_client: GatewayClient,
    node_client: NodeClient,
    payment_db: Mutex<PaymentDb<FlatFileFs>>,

    /// We only want one task syncing payments at a time. Ideally the dart side
    /// shouldn't let this happen, but just to be safe let's add this in.
    payment_sync_lock: tokio::sync::Mutex<()>,

    /// App settings
    settings_db: Arc<SettingsDb>,

    /// Some misc. info needed for user support / user account deletion.
    user_info: AppUserInfoRs,
}

impl App {
    /// Signup a new user.
    ///
    /// - `gdrive_signup_creds`: set to `Some` if the user is signing up with
    ///   active Google Drive backup.
    #[instrument(skip_all, name = "(signup)")]
    pub async fn signup(
        rng: &mut impl Crng,
        config: AppConfig,
        root_seed: &RootSeed,
        gdrive_signup_creds: Option<GDriveSignupCredentials>,
        signup_code: Option<String>,
        partner: Option<UserPk>,
    ) -> anyhow::Result<Self> {
        // derive user key and node key
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk = NodePk(node_key_pair.public_key());
        let user_config = UserAppConfig::new(config, user_pk);

        // gen + sign the UserSignupRequestWireV1
        let node_pk_proof = NodePkProof::sign(rng, &node_key_pair);
        let user_info = AppUserInfoRs {
            user_pk,
            node_pk,
            node_pk_proof: node_pk_proof.clone(),
        };
        let signup_req = UserSignupRequestWire::V2(UserSignupRequestWireV2 {
            v1: UserSignupRequestWireV1 {
                node_pk_proof,
                signup_code,
            },
            partner,
        });
        let (_, signed_signup_req) = user_key_pair
            .sign_struct(&signup_req)
            .expect("Should never fail to serialize UserSignupRequestWire");

        // build NodeClient, GatewayClient
        let gateway_client = GatewayClient::new(
            user_config.config.deploy_env,
            user_config.config.gateway_url.clone(),
            user_config.config.user_agent.clone(),
        )
        .context("Failed to build GatewayClient")?;
        let node_client = NodeClient::new(
            rng,
            user_config.config.use_sgx,
            user_config.config.deploy_env,
            gateway_client.clone(),
            Credentials::from_root_seed(root_seed),
        )
        .context("Failed to build NodeClient")?;

        // Create new provision DB
        let provision_ffs =
            FlatFileFs::create_clean_dir_all(user_config.provision_db_dir())
                .context("Could not create provision ffs")?;

        // Create new settings DB
        let settings_ffs =
            FlatFileFs::create_clean_dir_all(user_config.settings_db_dir())
                .context("Could not create settings ffs")?;
        let settings_db = Arc::new(SettingsDb::load(settings_ffs));

        // Create new payments DB
        let payments_ffs =
            FlatFileFs::create_clean_dir_all(user_config.payment_db_dir())
                .context("Could not create payments ffs")?;
        let payment_db = Mutex::new(PaymentDb::empty(payments_ffs));

        // Create new provision history
        let provision_history = ProvisionHistory::new();

        // TODO(phlip9): retries?

        // signup the user and get the current releases
        let (try_signup, try_current_releases) = tokio::join!(
            gateway_client.signup_v2(&signed_signup_req),
            gateway_client.current_releases(),
        );
        try_signup.context("Failed to signup user")?;
        let current_releases =
            try_current_releases.context("Could not fetch current releases")?;

        // Provision the node for the first time and update latest_provisioned.
        // NOTE: This computes 600K HMAC iterations! We only do this at signup.
        let allow_gvfs_access = true;
        let maybe_encrypted_seed = gdrive_signup_creds
            .as_ref()
            .map(|c| &c.password)
            .map(|pass| root_seed.password_encrypt(rng, pass))
            .transpose()
            .context("Could not encrypt root seed under password")?;

        // This will provision all recent releases.
        let releases_to_provision = provision_history.releases_to_provision(
            user_config.config.deploy_env,
            current_releases,
        );

        let google_auth_code = gdrive_signup_creds.map(|c| c.server_auth_code);
        helpers::provision(
            provision_ffs,
            node_client.clone(),
            user_config.clone(),
            helpers::clone_root_seed(root_seed),
            provision_history,
            releases_to_provision,
            google_auth_code,
            allow_gvfs_access,
            maybe_encrypted_seed,
        )
        .await
        .context("Initial provision failed")?;

        // We've successfully signed up and provisioned our node; we can finally
        // "commit" and persist our root seed
        let secret_store = SecretStore::new(&user_config.config);
        secret_store
            .write_root_seed(root_seed)
            .context("Failed to persist root seed")?;

        info!(%user_pk, %node_pk, "new user signed up and node provisioned");
        Ok(Self {
            node_client,
            gateway_client,
            payment_db,
            payment_sync_lock: tokio::sync::Mutex::new(()),
            settings_db,
            user_info,
        })
    }

    /// Try to load the root seed from the platform secret store and app state
    /// from the local storage. Returns `None` if this is the first run.
    #[instrument(skip_all, name = "(load)")]
    pub async fn load<R: Crng>(
        rng: &mut R,
        config: AppConfig,
    ) -> anyhow::Result<Option<Self>> {
        let secret_store = SecretStore::new(&config);
        let maybe_root_seed = secret_store
            .read_root_seed()
            .context("Failed to read root seed from SecretStore")?;

        // If there's nothing in the secret store, this must be a fresh install;
        // we can just return here.
        let root_seed = match maybe_root_seed {
            None => return Ok(None),
            Some(s) => s,
        };

        // Derive and add user_pk to config
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let user_config = UserAppConfig::new(config, user_pk);
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let user_info = AppUserInfoRs::new(rng, user_pk, &node_key_pair);

        // Init API clients
        let gateway_client = GatewayClient::new(
            user_config.config.deploy_env,
            user_config.config.gateway_url.clone(),
            user_config.config.user_agent.clone(),
        )
        .context("Failed to build GatewayClient")?;
        let node_client = NodeClient::new(
            rng,
            user_config.config.use_sgx,
            user_config.config.deploy_env,
            gateway_client.clone(),
            Credentials::from_root_seed(&root_seed),
        )
        .context("Failed to build NodeClient")?;

        // Load provision DB
        let provision_ffs =
            FlatFileFs::create_dir_all(user_config.provision_db_dir())
                .context("Could not create provision ffs")?;

        // Load settings DB
        let settings_ffs =
            FlatFileFs::create_dir_all(user_config.settings_db_dir())
                .context("Could not create settings ffs")?;
        let settings_db = Arc::new(SettingsDb::load(settings_ffs));

        // Load payments DB
        let payments_ffs =
            FlatFileFs::create_dir_all(user_config.payment_db_dir())
                .context("Could not create payments ffs")?;
        let payment_db = PaymentDb::read(payments_ffs)
            .context("Failed to load payment db")?
            .apply(Mutex::new);

        // Load provision history
        let provision_history = ProvisionHistory::read_from_ffs(&provision_ffs)
            .context("Could not read provision history")?;
        match provision_history.provisioned.last() {
            Some(latest) => info!(
                version = %latest.version, measurement = %latest.measurement,
                "Latest provisioned: "
            ),
            None => info!("Empty provision history"),
        }

        // Fetch the current releases.
        let current_releases = gateway_client
            .current_releases()
            .await
            .context("Could not fetch current releases")?;

        // Provision all recent releases we haven't already provisioned
        let releases_to_provision = provision_history.releases_to_provision(
            user_config.config.deploy_env,
            current_releases,
        );

        if !releases_to_provision.is_empty() {
            info!("Provisioning releases: {releases_to_provision:?}");
            let google_auth_code = None;
            let allow_gvfs_access = true;
            // To avoid computing 600K HMAC iterations on every node upgrade,
            // we only pass an encrypted seed during `signup`.
            let maybe_encrypted_seed = None;
            helpers::provision(
                provision_ffs,
                node_client.clone(),
                user_config,
                helpers::clone_root_seed(&root_seed),
                provision_history,
                releases_to_provision,
                google_auth_code,
                allow_gvfs_access,
                maybe_encrypted_seed,
            )
            .await
            .context("Re-provision(s) failed")?;
        } else {
            info!("Already provisioned to all recent releases")
        }

        {
            let node_pk = root_seed.derive_node_pk(rng);
            let locked_payment_db = payment_db.lock().unwrap();
            let db_state = locked_payment_db.state();
            info!(
                %user_pk,
                %node_pk,
                num_payments = db_state.num_payments(),
                num_pending = db_state.num_pending(),
                latest_payment_index = ?db_state.latest_payment_index(),
                "loaded existing app state"
            );
        }

        Ok(Some(Self {
            gateway_client,
            node_client,
            payment_db,
            payment_sync_lock: tokio::sync::Mutex::new(()),
            settings_db,
            user_info,
        }))
    }

    /// Restore wallet from backup.
    ///
    /// `google_auth_code`: see [`NodeProvisionRequest::google_auth_code`]
    #[instrument(skip_all, name = "(restore)")]
    pub async fn restore(
        rng: &mut impl Crng,
        config: AppConfig,
        google_auth_code: Option<String>,
        root_seed: &RootSeed,
    ) -> anyhow::Result<Self> {
        // derive user key and node key
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk = NodePk(node_key_pair.public_key());
        let user_config = UserAppConfig::new(config, user_pk);
        let user_info = AppUserInfoRs::new(rng, user_pk, &node_key_pair);

        // build NodeClient, GatewayClient
        let gateway_client = GatewayClient::new(
            user_config.config.deploy_env,
            user_config.config.gateway_url.clone(),
            user_config.config.user_agent.clone(),
        )
        .context("Failed to build GatewayClient")?;
        let node_client = NodeClient::new(
            rng,
            user_config.config.use_sgx,
            user_config.config.deploy_env,
            gateway_client.clone(),
            Credentials::from_root_seed(root_seed),
        )
        .context("Failed to build NodeClient")?;

        // Create new provision DB
        let provision_ffs =
            FlatFileFs::create_clean_dir_all(user_config.provision_db_dir())
                .context("Could not create provision ffs")?;

        // Potentially restore settings DB
        let settings_ffs =
            FlatFileFs::create_dir_all(user_config.settings_db_dir())
                .context("Could not create settings ffs")?;
        let settings_db = Arc::new(SettingsDb::load(settings_ffs));

        // Create new payments DB
        let payments_ffs =
            FlatFileFs::create_clean_dir_all(user_config.payment_db_dir())
                .context("Could not create payments ffs")?;
        let payment_db = Mutex::new(PaymentDb::empty(payments_ffs));

        // Ask gateway for current releases
        let current_releases = gateway_client
            .current_releases()
            .await
            .context("Could not fetch current releases")?;

        // We don't have a provision history, so provision credentials to all
        // recent node versions.
        let allow_gvfs_access = true;
        let maybe_encrypted_seed = None;
        let provision_history = ProvisionHistory::new();
        let releases_to_provision = provision_history.releases_to_provision(
            user_config.config.deploy_env,
            current_releases,
        );
        helpers::provision(
            provision_ffs,
            node_client.clone(),
            user_config.clone(),
            helpers::clone_root_seed(root_seed),
            provision_history,
            releases_to_provision,
            google_auth_code,
            allow_gvfs_access,
            maybe_encrypted_seed,
        )
        .await
        .context("Re-provision failed")?;
        info!("Successfully re-provisioned to latest releases");

        // We've successfully restored and provisioned our node; we can finally
        // "commit" and persist our root seed
        let secret_store = SecretStore::new(&user_config.config);
        secret_store
            .write_root_seed(root_seed)
            .context("Failed to persist root seed")?;

        info!(%user_pk, %node_pk, "restored user");
        Ok(Self {
            node_client,
            gateway_client,
            payment_db,
            payment_sync_lock: tokio::sync::Mutex::new(()),
            settings_db,
            user_info,
        })
    }

    pub fn node_client(&self) -> &NodeClient {
        &self.node_client
    }

    pub fn gateway_client(&self) -> &GatewayClient {
        &self.gateway_client
    }

    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn settings_db(&self) -> Arc<SettingsDb> {
        self.settings_db.clone()
    }

    // TODO(phlip9): unhack this API when I figure out how to make frb stop auto
    // opaque'ing `AppUserInfo`.
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) fn user_info(&self) -> (String, String, String) {
        self.user_info.to_ffi()
    }

    // We have to hold the std Mutex lock past .await because of FRB
    #[allow(clippy::await_holding_lock)]
    #[instrument(skip_all, name = "(sync-payments)")]
    pub async fn sync_payments(&self) -> anyhow::Result<PaymentSyncSummary> {
        let start = Instant::now();
        info!("start");

        let res = {
            // Ensure only one task syncs payments at-a-time
            let _lock = match self.payment_sync_lock.try_lock() {
                Ok(lock) => lock,
                Err(_) =>
                    return Err(anyhow!(
                        "Another tasking is currently syncing payments!"
                    )),
            };

            payments::sync_payments(
                &self.payment_db,
                &self.node_client,
                constants::DEFAULT_PAYMENTS_BATCH_SIZE,
            )
            .await
        };

        let elapsed = start.elapsed();
        match &res {
            Ok(summary) => info!("success: elapsed: {elapsed:?}, {summary:?}"),
            Err(err) => warn!("error: elapsed: {elapsed:?}, {err:#?}"),
        }

        res
    }

    pub fn payment_db(&self) -> &Mutex<PaymentDb<FlatFileFs>> {
        &self.payment_db
    }
}

mod helpers {
    use super::*;

    /// Helper to provision to the given releases and update the
    /// provision history.
    ///
    /// - `allow_gvfs_access`: See [`NodeProvisionRequest::allow_gvfs_access`].
    /// - `google_auth_code`: See [`NodeProvisionRequest::google_auth_code`].
    /// - `maybe_encrypted_seed`: See [`NodeProvisionRequest::encrypted_seed`].
    pub(super) async fn provision(
        provision_ffs: impl Ffs + Clone + Send + Sync + 'static,
        node_client: NodeClient,
        user_config: UserAppConfig,
        root_seed: RootSeed,
        mut provision_history: ProvisionHistory,
        mut releases_to_provision: BTreeSet<NodeRelease>,
        google_auth_code: Option<String>,
        allow_gvfs_access: bool,
        encrypted_seed: Option<Vec<u8>>,
    ) -> anyhow::Result<()> {
        info!("Starting provisioning: {releases_to_provision:?}");

        /// Provisions a single release and updates the provision history.
        async fn provision_inner(
            provision_ffs: &impl Ffs,
            node_client: &NodeClient,
            user_config: &UserAppConfig,
            provision_history: &mut ProvisionHistory,
            root_seed: RootSeed,
            release: NodeRelease,
            google_auth_code: Option<String>,
            allow_gvfs_access: bool,
            // TODO(max): We could have cheaper cloning by using Bytes here
            encrypted_seed: Option<Vec<u8>>,
        ) -> anyhow::Result<()> {
            let provision_req = NodeProvisionRequest {
                root_seed,
                deploy_env: user_config.config.deploy_env,
                network: user_config.config.network,
                google_auth_code,
                allow_gvfs_access,
                encrypted_seed,
            };
            node_client
                .provision(release.measurement, provision_req)
                .await
                .context("Failed to provision node")?;

            // Provision success; Mark this release as provisioned
            provision_history
                .update_and_persist(release.clone(), provision_ffs)
                .context("Could not add to provision history")?;

            info!(
                version = %release.version,
                measurement = %release.measurement,
                "Provision success:"
            );

            Ok(())
        }

        // Make sure the latest trusted version is provisioned before we return,
        // so that when we request a node run, Lexe runs the latest version.
        let latest = match releases_to_provision.pop_last() {
            Some(release) => release,
            None => {
                info!("No releases to provision");
                return Ok(());
            }
        };

        // Provision the latest trusted release inline
        provision_inner(
            &provision_ffs,
            &node_client,
            &user_config,
            &mut provision_history,
            helpers::clone_root_seed(&root_seed),
            latest,
            google_auth_code.clone(),
            allow_gvfs_access,
            encrypted_seed.clone(),
        )
        .await?;

        // Early return if no work left to do
        if releases_to_provision.is_empty() {
            return Ok(());
        }

        // Provision remaining versions asynchronously so that we don't block
        // app startup.

        // TODO(max): In the future we may want to drive the secondary
        // provisioning in function calls instead of background tasks. Some sage
        // advice from wizard Philip:
        //
        // """
        // I've found that structuring everything as function calls driven by
        // the flutter frontend to the app-rs library ends up being the
        // best approach in the end.
        //
        // - The flutter frontend owns the page and app lifecycle, best
        //   understands what calls and services are relevant, and trying to
        //   keep that in sync with Rust is cumbersome.
        // - It's much easier to mock out RPC-style fn calls for design work.
        // - Reporting errors to the user is also easy, since the error gets
        //   bubbled up to the frontend to display.
        // - If a background task has an error, there's no clear way to report
        //   to the user, so you just log and things are silently broken.
        // """
        const SPAN_NAME: &str = "(secondary-provision)";
        let task = LxTask::spawn_with_span(
            SPAN_NAME,
            info_span!(SPAN_NAME),
            async move {
                // NOTE: We provision releases serially because each provision
                // updates the approved versions list, and we don't currently
                // have a locking mechanism.
                for node_release in releases_to_provision {
                    let provision_result = provision_inner(
                        &provision_ffs,
                        &node_client,
                        &user_config,
                        &mut provision_history,
                        helpers::clone_root_seed(&root_seed),
                        node_release.clone(),
                        google_auth_code.clone(),
                        allow_gvfs_access,
                        encrypted_seed.clone(),
                    )
                    .await;

                    if let Err(e) = provision_result {
                        warn!(
                            version = %node_release.version,
                            measurement = %node_release.measurement,
                            "Secondary provision failed: {e:#}"
                        );
                    }
                }

                info!("Secondary provisioning complete");
            },
        );

        // TODO(max): Ideally, we could await on this ephemeral task somewhere
        // for structured concurrency. But not sure if it even matters, as the
        // mobile OS will often just kill the app.
        task.detach();

        Ok(())
    }

    /// Clone a RootSeed reference into a new RootSeed instance.
    // TODO(phlip9): we should get rid of this helper eventually. We could
    // use something like a `Cow<'a, &RootSeed>` in `NodeProvisionRequest`. Ofc
    // we still have the seed serialized in a heap-allocated json blob when we
    // make the request, which is much harder for us to zeroize...
    pub(super) fn clone_root_seed(root_seed_ref: &RootSeed) -> RootSeed {
        RootSeed::new(Secret::new(*root_seed_ref.expose_secret()))
    }
}

/// Pure-Rust configuration for a particular user app.
#[derive(Clone)]
pub struct AppConfig {
    pub deploy_env: DeployEnv,
    pub network: LxNetwork,
    pub use_sgx: bool,
    pub gateway_url: String,
    pub base_app_data_dir: PathBuf,
    pub use_mock_secret_store: bool,
    pub user_agent: String,
}

impl AppConfig {
    // `<base_app_data_dir>/<deploy_env>-<network>-<use_sgx>`
    pub(crate) fn app_data_dir(&self) -> PathBuf {
        self.base_app_data_dir.join(self.build_flavor().to_string())
    }

    // `<base_app_data_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>`
    fn user_data_dir(&self, user_pk: &UserPk) -> PathBuf {
        self.app_data_dir().join(user_pk.to_string())
    }

    pub fn build_flavor(&self) -> BuildFlavor {
        BuildFlavor {
            deploy_env: self.deploy_env,
            network: self.network,
            use_sgx: self.use_sgx,
        }
    }

    #[cfg(feature = "flutter")]
    pub(crate) fn from_dart_config(
        deploy_env: DeployEnv,
        network: LxNetwork,
        gateway_url: String,
        use_sgx: bool,
        base_app_data_dir: String,
        use_mock_secret_store: bool,
        user_agent: String,
    ) -> Self {
        let build = BuildFlavor {
            deploy_env,
            network,
            use_sgx,
        };

        // The base app data directory.
        // See: dart fn `path_provider.getApplicationSupportDirectory()`
        // https://pub.dev/documentation/path_provider/latest/path_provider/getApplicationSupportDirectory.html
        let base_app_data_dir = PathBuf::from(base_app_data_dir);

        {
            use DeployEnv::*;
            match (deploy_env, network, use_sgx, use_mock_secret_store) {
                (Prod, LxNetwork::Mainnet, true, false) => (),
                (Staging, LxNetwork::Testnet3, true, false) => (),
                (Staging, LxNetwork::Testnet4, true, false) => (),
                (Dev, LxNetwork::Testnet3, _, _)
                | (Dev, LxNetwork::Testnet4, _, _)
                | (Dev, LxNetwork::Regtest, _, _) => (),
                _ => panic!("Unsupported app config combination: {build}"),
            }
        }

        Self {
            deploy_env,
            network,
            gateway_url,
            use_sgx,
            base_app_data_dir,
            use_mock_secret_store,
            user_agent,
        }
    }
}

/// Wraps a [`AppConfig`] to include user-specific data.
#[derive(Clone)]
struct UserAppConfig {
    config: AppConfig,
    user_data_dir: PathBuf,
}

impl UserAppConfig {
    fn new(config: AppConfig, user_pk: UserPk) -> Self {
        let user_data_dir = config.user_data_dir(&user_pk);
        Self {
            config,
            user_data_dir,
        }
    }

    fn provision_db_dir(&self) -> PathBuf {
        self.user_data_dir.join("provision_db")
    }

    fn payment_db_dir(&self) -> PathBuf {
        self.user_data_dir.join("payment_db")
    }

    fn settings_db_dir(&self) -> PathBuf {
        self.user_data_dir.join("settings_db")
    }
}

/// An app build variant / flavor. We use this struct to disambiguate persisted
/// state and secrets so we don't accidentally clobber state when testing across
/// e.g. testnet vs regtest.
#[derive(Copy, Clone)]
pub struct BuildFlavor {
    deploy_env: DeployEnv,
    network: LxNetwork,
    use_sgx: bool,
}

impl fmt::Display for BuildFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let deploy_env = self.deploy_env.as_str();
        let network = self.network.as_str();
        let sgx = if self.use_sgx { "sgx" } else { "dbg" };
        write!(f, "{deploy_env}-{network}-{sgx}")
    }
}

/// Some assorted user/node info. This is kinda hacked together currently just
/// to support account deletion requests.
struct AppUserInfoRs {
    pub user_pk: UserPk,
    pub node_pk: NodePk,
    pub node_pk_proof: NodePkProof,
}

impl AppUserInfoRs {
    fn new<R: Crng>(
        rng: &mut R,
        user_pk: UserPk,
        node_key_pair: &secp256k1::Keypair,
    ) -> Self {
        let node_pk = NodePk(node_key_pair.public_key());
        let node_pk_proof = NodePkProof::sign(rng, node_key_pair);
        Self {
            user_pk,
            node_pk,
            node_pk_proof,
        }
    }

    // NOTE(phlip9): I can't for the life of me figure out why frb keeps trying
    // to RustAutoOpaque wrap the ffi type. Impling the conversion here seems to
    // make it stop???
    fn to_ffi(&self) -> (String, String, String) {
        let user_pk = self.user_pk.to_string();
        let node_pk = self.node_pk.to_string();
        let node_pk_proof = self.node_pk_proof.to_hex_string();
        (user_pk, node_pk, node_pk_proof)
    }
}
