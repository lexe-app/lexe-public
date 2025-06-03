//! The Rust native app state. The interfaces here should look like standard
//! Rust, without any FFI weirdness.

use std::{
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
use secrecy::ExposeSecret;
use tracing::{info, instrument, warn};

use crate::{
    client::{Credentials, GatewayClient, NodeClient},
    ffs::{Ffs, FlatFileFs},
    payments::{self, PaymentDb, PaymentSyncSummary},
    secret_store::SecretStore,
    settings::SettingsDb,
    storage,
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
        let user_config = AppConfigWithUserPk::new(config, user_pk);
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

        // See if there is a newer version we haven't provisioned to yet.
        // If so, re-provision to it and update the latest_provisioned file.
        let maybe_latest_provisioned =
            storage::read_latest_provisioned(&provision_ffs)
                .context("Colud not read latest provisioned")?;
        match &maybe_latest_provisioned {
            Some(x) =>
                info!(version = %x.version, measurement = %x.measurement, "latest provisioned"),
            None =>
                warn!("Could not find latest provisioned file. Was it deleted?"),
        }

        let latest_release = gateway_client
            .latest_release()
            .await
            .context("Could not fetch latest release")?;
        info!(
            version = %latest_release.version,
            measurement = %latest_release.measurement,
            "latest release",
        );

        // TODO(max): Ensure that user has approved this version before
        // proceeding to re-provision.
        let do_reprovision = match maybe_latest_provisioned {
            // Compare `semver::Version`s.
            Some(latest_provisioned) =>
                latest_provisioned.version < latest_release.version,
            // If there is no latest provision release, just (re-)provision.
            None => true,
        };
        if do_reprovision {
            // TODO(max): We might want to ask Lexe if our GDriveCredentials are
            // currently working. If not, we should run the user through the
            // oauth flow again then pass this as Some().
            let google_auth_code = None;
            // TODO(max): We should probably check whether the root seed backup
            // already exists before proceeding to set this as None. Or if we
            // have access to the password somewhere, we could always set this
            // to Some(_) to ensure the user always has a root seed backup.
            let password = None;
            Self::do_provision(
                rng,
                &node_client,
                &latest_release,
                &user_config,
                &root_seed,
                &provision_ffs,
                google_auth_code,
                password,
            )
            .await
            .context("Re-provision failed")?;
            info!("Successfully re-provisioned to latest release");
        } else {
            info!("Already provisioned to latest release")
        }

        {
            let node_pk = root_seed.derive_node_pk(rng);
            let lock = payment_db.lock().unwrap();
            let db_state = lock.state();
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
        let user_config = AppConfigWithUserPk::new(config, user_pk);
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

        // Ask gateway for latest release
        let latest_release = gateway_client
            .latest_release()
            .await
            .context("Could not fetch latest release")?;
        info!(
            version = %latest_release.version,
            measurement = %latest_release.measurement,
            "latest release",
        );

        // Reprovision credentials to most recent node version
        let password = None;
        Self::do_provision(
            rng,
            &node_client,
            &latest_release,
            &user_config,
            root_seed,
            &provision_ffs,
            google_auth_code,
            password,
        )
        .await
        .context("Re-provision failed")?;
        info!("Successfully re-provisioned to latest release");

        // we've successfully signed up and provisioned our node; we can finally
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

    /// Signup a new user.
    ///
    /// `google_auth_code`: see [`NodeProvisionRequest::google_auth_code`]
    /// `password`: see [`NodeProvisionRequest::encrypted_seed`]
    #[instrument(skip_all, name = "(signup)")]
    pub async fn signup(
        rng: &mut impl Crng,
        config: AppConfig,
        root_seed: &RootSeed,
        google_auth_code: Option<String>,
        password: Option<&str>,
        signup_code: Option<String>,
    ) -> anyhow::Result<Self> {
        // derive user key and node key
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk = NodePk(node_key_pair.public_key());
        let user_config = AppConfigWithUserPk::new(config, user_pk);

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
            partner: None,
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

        // TODO(phlip9): retries?

        // signup the user and get the latest release
        let (try_signup, try_latest_release) = tokio::join!(
            gateway_client.signup_v2(&signed_signup_req),
            gateway_client.latest_release(),
        );
        try_signup.context("Failed to signup user")?;
        let latest_release =
            try_latest_release.context("Could not fetch latest release")?;

        // Provision the node for the first time and update latest_provisioned.
        // TODO(max): Ensure that user has approved this version before
        // proceeding to re-provision.
        Self::do_provision(
            rng,
            &node_client,
            &latest_release,
            &user_config,
            root_seed,
            &provision_ffs,
            google_auth_code,
            password,
        )
        .await
        .context("First provision failed")?;

        // we've successfully signed up and provisioned our node; we can finally
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

    /// Provision to the given release and update the "latest_provisioned" file.
    async fn do_provision(
        rng: &mut impl Crng,
        node_client: &NodeClient,
        node_release: &NodeRelease,
        user_config: &AppConfigWithUserPk,
        root_seed: &RootSeed,
        app_data_ffs: &impl Ffs,
        google_auth_code: Option<String>,
        maybe_password: Option<&str>,
    ) -> anyhow::Result<()> {
        // TODO(phlip9): we could get rid of this extra RootSeed copy on the
        // stack by using something like a `Cow<'a, &RootSeed>` in
        // `NodeProvisionRequest`. Ofc we still have the seed serialized in a
        // heap-allocated json blob when we make the request, which is much
        // harder for us to zeroize...
        let root_seed_clone =
            RootSeed::new(Secret::new(*root_seed.expose_secret()));
        let encrypted_seed = maybe_password
            .map(|pass| root_seed.password_encrypt(rng, pass))
            .transpose()
            .context("Could not encrypt root seed under password")?;

        let provision_req = NodeProvisionRequest {
            root_seed: root_seed_clone,
            deploy_env: user_config.config.deploy_env,
            network: user_config.config.network,
            google_auth_code,
            allow_gvfs_access: true,
            encrypted_seed,
        };
        node_client
            .provision(node_release.measurement, provision_req)
            .await
            .context("Failed to provision node")?;

        storage::write_latest_provisioned(app_data_ffs, node_release)
            .context("Could not write latest provisioned")?;

        info!(
            version = %node_release.version,
            measurement = %node_release.measurement,
            "Provision success:"
        );
        Ok(())
    }
}

/// Pure-Rust configuration for a particular user app.
#[derive(Clone)]
pub struct AppConfig {
    pub deploy_env: DeployEnv,
    pub network: common::ln::network::LxNetwork,
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
        // See: dart fn [`path_provider.getApplicationSupportDirectory()`](https://pub.dev/documentation/path_provider/latest/path_provider/getApplicationSupportDirectory.html)
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

struct AppConfigWithUserPk {
    config: AppConfig,
    user_data_dir: PathBuf,
}

impl AppConfigWithUserPk {
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
