//! The Rust native app state. The interfaces here should look like standard
//! Rust, without any FFI weirdness.

use std::{
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{anyhow, Context};
use common::{
    api::{
        auth::{BearerAuthenticator, UserSignupRequest},
        def::{AppBackendApi, AppGatewayApi, AppNodeProvisionApi},
        models::NodeRelease,
        provision::NodeProvisionRequest,
        NodePk, NodePkProof, UserPk,
    },
    client::{GatewayClient, NodeClient},
    constants,
    rng::Crng,
    root_seed::RootSeed,
    Apply, Secret,
};
use secrecy::ExposeSecret;
use tracing::{info, instrument, warn};

use crate::{
    bindings::{Config, DeployEnv, Network},
    ffs::{Ffs, FlatFileFs},
    payments::{self, PaymentDb, PaymentSyncSummary},
    secret_store::SecretStore,
    storage,
};

pub struct App {
    gateway_client: GatewayClient,
    node_client: NodeClient,
    payment_db: Mutex<PaymentDb<FlatFileFs>>,

    /// We only want one task syncing payments at a time. Ideally the dart side
    /// shouldn't let this happen, but just to be safe let's add this in.
    // ideally this could just be a tokio::sync::Mutex, but those aren't
    // Unwind-safe, which flutter_rust_bridge requires, etc etc...
    payment_sync_lock: Mutex<()>,
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

        // Init API clients
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = *user_key_pair.public_key();
        let bearer_authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));
        let gateway_client = GatewayClient::new(config.gateway_url.clone());
        let node_client = NodeClient::new(
            rng,
            config.use_sgx,
            &root_seed,
            config.deploy_env.into(),
            bearer_authenticator,
            gateway_client.clone(),
        )
        .context("Failed to build NodeClient")?;

        // Init local storage
        let app_data_ffs =
            FlatFileFs::create_dir_all(config.app_data_dir.clone())
                .context("Could not create app data ffs")?;
        let payments_ffs = FlatFileFs::create_dir_all(config.payment_db_dir())
            .context("Could not create payments ffs")?;
        let payment_db = PaymentDb::read(payments_ffs)
            .context("Failed to load payment db")?
            .apply(Mutex::new);

        // See if there is a newer version we haven't provisioned to yet.
        // If so, re-provision to it and update the latest_provisioned file.
        let maybe_latest_provisioned =
            storage::read_latest_provisioned(&app_data_ffs)
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
                &config,
                &root_seed,
                &app_data_ffs,
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
            payment_sync_lock: Mutex::new(()),
        }))
    }

    #[instrument(skip_all, name = "(restore)")]
    pub async fn restore(
        _config: AppConfig,
        _seed_phrase: String,
    ) -> anyhow::Result<Self> {
        todo!()
    }

    pub async fn signup(
        rng: &mut impl Crng,
        config: AppConfig,
        google_auth_code: String,
        password: String,
    ) -> anyhow::Result<Self> {
        // sample a new RootSeed
        let root_seed = RootSeed::from_rng(rng);

        // TODO(phlip9): what if we need to retry signup? we should probably
        // check SecretStore defensively to see if we've already signed up and
        // stored a RootSeed.

        // single-use `serverAuthCode` from Google OAuth 2 consent flow, used by
        // the enclave to get access+refresh tokens.
        let google_auth_code = Some(google_auth_code);

        // The root seed backup password.
        let password = Some(password);

        Self::signup_custom(rng, config, root_seed, google_auth_code, password)
            .await
    }

    /// Allows signing up with a specific [`RootSeed`], useful in tests.
    #[instrument(skip_all, name = "(signup)")]
    pub async fn signup_custom(
        rng: &mut impl Crng,
        config: AppConfig,
        root_seed: RootSeed,
        google_auth_code: Option<String>,
        maybe_password: Option<String>,
    ) -> anyhow::Result<Self> {
        // derive user key and node key
        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk = NodePk(node_key_pair.public_key());

        // gen + sign the UserSignupRequest
        let node_pk_proof = NodePkProof::sign(rng, &node_key_pair);
        let signup_req = UserSignupRequest { node_pk_proof };
        let (_, signed_signup_req) = user_key_pair
            .sign_struct(&signup_req)
            .expect("Should never fail to serialize UserSignupRequest");

        // build NodeClient, GatewayClient
        let bearer_authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));
        let gateway_client = GatewayClient::new(config.gateway_url.clone());
        let node_client = NodeClient::new(
            rng,
            config.use_sgx,
            &root_seed,
            config.deploy_env.into(),
            bearer_authenticator,
            gateway_client.clone(),
        )
        .context("Failed to build NodeClient")?;

        // Init local storage
        let app_data_ffs =
            FlatFileFs::create_dir_all(config.app_data_dir.clone())
                .context("Could not create app data ffs")?;
        let payments_ffs =
            FlatFileFs::create_clean_dir_all(config.payment_db_dir())
                .context("Could not create payments ffs")?;
        let payment_db = Mutex::new(PaymentDb::empty(payments_ffs));

        // TODO(phlip9): retries?

        // signup the user and get the latest release
        let (try_signup, try_latest_release) = tokio::join!(
            gateway_client.signup(signed_signup_req.cloned()),
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
            &config,
            &root_seed,
            &app_data_ffs,
            google_auth_code,
            maybe_password.as_deref(),
        )
        .await
        .context("First provision failed")?;

        // TODO(phlip9): commit RootSeed earlier?

        // we've successfully signed up and provisioned our node; we can finally
        // "commit" and persist our root seed
        let secret_store = SecretStore::new(&config);
        secret_store
            .write_root_seed(&root_seed)
            .context("Failed to persist root seed")?;

        info!(
            %user_pk,
            %node_pk,
            "new user signed up and node provisioned"
        );

        Ok(Self {
            node_client,
            gateway_client,
            payment_db,
            payment_sync_lock: Mutex::new(()),
        })
    }

    pub fn node_client(&self) -> &NodeClient {
        &self.node_client
    }

    pub fn gateway_client(&self) -> &GatewayClient {
        &self.gateway_client
    }

    // We have to hold the std Mutex lock past .await because of FRB
    #[allow(clippy::await_holding_lock)]
    #[instrument(skip_all, name = "(sync_payments)")]
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
        config: &AppConfig,
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
            deploy_env: config.deploy_env.into(),
            network: config.network,
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

        Ok(())
    }
}

/// Pure-Rust configuration for a particular user app.
pub struct AppConfig {
    pub deploy_env: DeployEnv,
    pub network: common::cli::Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub app_data_dir: PathBuf,
    pub use_mock_secret_store: bool,
}

impl AppConfig {
    pub fn payment_db_dir(&self) -> PathBuf {
        self.app_data_dir.join("payment_db")
    }

    pub fn build_flavor(&self) -> BuildFlavor {
        BuildFlavor {
            deploy_env: self.deploy_env,
            network: self.network,
            use_sgx: self.use_sgx,
        }
    }
}

impl From<Config> for AppConfig {
    fn from(config: Config) -> Self {
        use DeployEnv::*;
        use Network::*;

        let deploy_env = config.deploy_env;
        let network = config.network;
        let gateway_url = config.gateway_url;
        let use_sgx = config.use_sgx;
        let build = BuildFlavor {
            deploy_env,
            network: network.into(),
            use_sgx,
        };

        // The base app data directory.
        // See: dart fn [`path_provider.getApplicationSupportDirectory()`](https://pub.dev/documentation/path_provider/latest/path_provider/getApplicationSupportDirectory.html)
        let base_app_data_dir = PathBuf::from(config.base_app_data_dir);
        // To make development easier and avoid mixing state across
        // environments, we'll shove everything into a disambiguated subdir for
        // each environment/network pair, e.g., "prod-mainnet-sgx",
        // "dev-regtest-dbg".
        let app_data_dir = base_app_data_dir.join(build.to_string());

        let use_mock_secret_store = config.use_mock_secret_store;

        match (&deploy_env, &network, use_sgx, use_mock_secret_store) {
            (Prod, Mainnet, true, false) => (),
            (Staging, Testnet, true, false) => (),
            (Dev, Testnet, _, _) | (Dev, Regtest, _, _) => (),
            _ => panic!("Unsupported app config combination: {build}"),
        }

        Self {
            deploy_env,
            network: network.into(),
            gateway_url,
            use_sgx,
            app_data_dir,
            use_mock_secret_store,
        }
    }
}

/// An app build variant / flavor. We use this struct to disambiguate persisted
/// state and secrets so we don't accidentally clobber state when testing across
/// e.g. testnet vs regtest.
#[derive(Clone, Copy)]
pub struct BuildFlavor {
    network: common::cli::Network,
    deploy_env: DeployEnv,
    use_sgx: bool,
}

impl fmt::Display for BuildFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let network = self.network;
        let env = self.deploy_env.as_str();
        let sgx = if self.use_sgx { "sgx" } else { "dbg" };
        write!(f, "{network}-{env}-{sgx}")
    }
}
