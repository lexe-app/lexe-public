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
        def::{AppBackendApi, AppNodeProvisionApi},
        provision::NodeProvisionRequest,
        NodePk, NodePkProof, UserPk,
    },
    attest,
    client::{tls::dummy_lexe_ca_cert, GatewayClient, NodeClient},
    constants, enclave,
    rng::Crng,
    root_seed::RootSeed,
    Secret,
};
use secrecy::ExposeSecret;
use tracing::{info, instrument, warn};

use crate::{
    bindings::{Config, DeployEnv, Network},
    payments::{self, FlatFileFs, PaymentDb, PaymentSyncSummary},
    secret_store::SecretStore,
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

        // TODO(phlip9): load expected measurement from user settings
        let measurement = enclave::MOCK_MEASUREMENT;

        let enclave_policy = attest::EnclavePolicy {
            allow_debug: config.allow_debug_enclaves,
            trusted_mrenclaves: Some(vec![measurement]),
            // TODO(phlip9): load expected lexe signer from build config
            trusted_mrsigner: None,
        };
        let attest_verifier = attest::ServerCertVerifier {
            expect_dummy_quote: !config.use_sgx,
            enclave_policy,
        };

        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = *user_key_pair.public_key();
        let bearer_authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));

        let gateway_client = GatewayClient::new(config.gateway_url.clone());

        let node_client = NodeClient::new(
            rng,
            &root_seed,
            bearer_authenticator,
            gateway_client.clone(),
            &dummy_lexe_ca_cert(),
            attest_verifier,
            constants::NODE_PROVISION_HTTPS,
            constants::NODE_RUN_HTTPS,
        )
        .context("Failed to build NodeClient")?;

        let flat_fs = FlatFileFs::create_dir_all(config.payment_db_dir())?;
        let payment_db = Mutex::new(
            PaymentDb::read(flat_fs).context("Failed to load payment db")?,
        );

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

    pub async fn restore(
        _config: AppConfig,
        _seed_phrase: String,
    ) -> anyhow::Result<Self> {
        todo!()
    }

    pub async fn signup<R: Crng>(
        rng: &mut R,
        config: AppConfig,
    ) -> anyhow::Result<Self> {
        let gateway_url = config.gateway_url.clone();
        let use_sgx = config.use_sgx;

        // TODO: query backend (via gateway) for latest measurement
        let measurement = enclave::MOCK_MEASUREMENT;

        // sample the RootSeed

        let root_seed = RootSeed::from_rng(rng);

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

        // build NodeClient

        let enclave_policy = attest::EnclavePolicy {
            allow_debug: config.allow_debug_enclaves,
            trusted_mrenclaves: Some(vec![measurement]),
            // TODO(phlip9): load expected lexe signer from build config
            trusted_mrsigner: None,
        };
        let attest_verifier = attest::ServerCertVerifier {
            expect_dummy_quote: !use_sgx,
            enclave_policy,
        };

        let bearer_authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));

        let gateway_client = GatewayClient::new(gateway_url);

        let node_client = NodeClient::new(
            rng,
            &root_seed,
            bearer_authenticator,
            gateway_client.clone(),
            &dummy_lexe_ca_cert(),
            attest_verifier,
            constants::NODE_PROVISION_HTTPS,
            constants::NODE_RUN_HTTPS,
        )
        .context("Failed to build NodeClient")?;

        // TODO(phlip9): retries?

        // signup the user

        gateway_client
            .signup(signed_signup_req.cloned())
            .await
            .context("Failed to signup user")?;

        // provision new node enclave

        // TODO(phlip9): we could get rid of this extra RootSeed copy on the
        // stack by using something like a `Cow<'a, &RootSeed>` in
        // `NodeProvisionRequest`. Ofc we still have the seed serialized in a
        // heap-allocated json blob when we make the request, which is much
        // harder for us to zeroize...
        let root_seed_clone =
            RootSeed::new(Secret::new(*root_seed.expose_secret()));

        node_client
            .provision(NodeProvisionRequest {
                user_pk,
                node_pk,
                root_seed: root_seed_clone,
            })
            .await
            .context("Failed to provision node")?;

        // we've successfully signed up and provisioned our node; we can finally
        // "commit" and persist our root seed

        let secret_store = SecretStore::new(&config);
        secret_store
            .write_root_seed(&root_seed)
            .context("Failed to persist root seed")?;

        let flat_fs =
            FlatFileFs::create_clean_dir_all(config.payment_db_dir())?;
        let payment_db = Mutex::new(PaymentDb::empty(flat_fs));

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
}

/// Pure-Rust configuration for a particular user app.
pub struct AppConfig {
    pub deploy_env: DeployEnv,
    pub network: common::cli::Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub allow_debug_enclaves: bool,
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
        let use_sgx = false;
        let build = BuildFlavor {
            deploy_env,
            network: network.into(),
            use_sgx,
        };

        let allow_debug_enclaves = deploy_env == Dev;

        // The base app data directory.
        // See: dart fn [`path_provider.getApplicationSupportDirectory()`](https://pub.dev/documentation/path_provider/latest/path_provider/getApplicationSupportDirectory.html)
        let base_app_data_dir = PathBuf::from(config.app_data_dir);
        // To make development easier and avoid mixing state across
        // environments, we'll shove everything into a disambiguated subdir for
        // each environment/network pair, e.g., "prod-mainnet-sgx",
        // "dev-regtest-dbg".
        let app_data_dir = base_app_data_dir.join(build.to_string());

        let use_mock_secret_store = config.use_mock_secret_store;

        match (&deploy_env, &network) {
            (Prod, Bitcoin) => todo!(),
            (Staging, Testnet) => todo!(),
            (Dev, Testnet) => todo!(),
            (Dev, Regtest) => Self {
                deploy_env,
                network: network.into(),
                gateway_url,
                use_sgx,
                allow_debug_enclaves,
                app_data_dir,
                use_mock_secret_store,
            },
            _ => panic!(
                "Bad app config combination: {deploy_env:?} build is not \
                 compatible with {network:?} network"
            ),
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
