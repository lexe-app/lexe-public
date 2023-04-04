//! The Rust native app state. The interfaces here should look like standard
//! Rust, without any FFI weirdness.

use std::sync::Arc;

use anyhow::Context;
use common::api::auth::{BearerAuthenticator, UserSignupRequest};
use common::api::def::{AppBackendApi, AppNodeProvisionApi};
use common::api::provision::NodeProvisionRequest;
use common::api::{NodePk, NodePkProof, UserPk};
use common::client::tls::dummy_lexe_ca_cert;
use common::client::NodeClient;
use common::rng::Crng;
use common::root_seed::RootSeed;
use common::{attest, constants, enclave, Secret};
use secrecy::ExposeSecret;

use crate::bindings::{Config, DeployEnv, Network};
use crate::secret_store::SecretStore;

pub struct App {
    secret_store: SecretStore,
    node_client: NodeClient,
}

impl App {
    /// Load the app state from local storage. Returns `None` if this is the
    /// first run.
    pub async fn load(_config: AppConfig) -> anyhow::Result<Option<Self>> {
        // TODO(phlip9): load from disk
        Ok(None)
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
            trusted_mrsigner: None,
        };
        let attest_verifier = attest::ServerCertVerifier {
            expect_dummy_quote: !use_sgx,
            enclave_policy,
        };

        let bearer_authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, None));

        let node_client = NodeClient::new(
            rng,
            &root_seed,
            bearer_authenticator,
            gateway_url,
            &dummy_lexe_ca_cert(),
            attest_verifier,
            constants::NODE_PROVISION_HTTPS,
            constants::NODE_RUN_HTTPS,
        )
        .context("Failed to build NodeClient")?;

        // TODO(phlip9): retries?

        // signup the user

        node_client
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

        let secret_store = SecretStore::new();
        secret_store
            .write_root_seed(&root_seed)
            .context("Failed to persist root seed")?;

        // TODO(phlip9): how to logs
        // info!("node_client.provision() success");

        Ok(Self {
            secret_store,
            node_client,
        })
    }

    pub fn client(&self) -> &NodeClient {
        &self.node_client
    }

    pub fn secret_store(&self) -> &SecretStore {
        &self.secret_store
    }
}

/// Pure-Rust configuration for a particular user app.
pub struct AppConfig {
    pub network: common::cli::Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub allow_debug_enclaves: bool,
}

impl From<Config> for AppConfig {
    fn from(config: Config) -> Self {
        use DeployEnv::*;
        use Network::*;

        let deploy_env = config.deploy_env;
        let network = config.network;

        // Since we might be running on a real mobile device, we need to
        // set this value at build time, on the host.
        // TODO: .env is not sourced at build time...
        // let build_gateway_url = std::env!("DEV_GATEWAY_URL");
        // let build_use_sgx = std::env!("SGX") == "true";
        let build_gateway_url = "http://phlipdesk.local:4040";
        let build_use_sgx = false;
        let allow_debug_enclaves = deploy_env == Dev;

        match (&deploy_env, &network) {
            (Prod, Bitcoin) => todo!(),
            (Staging, Testnet) => todo!(),
            (Dev, Testnet) => todo!(),
            (Dev, Regtest) => Self {
                network: network.into(),
                gateway_url: build_gateway_url.to_owned(),
                use_sgx: build_use_sgx,
                allow_debug_enclaves,
            },
            _ => panic!(
                "Bad app config combination: {deploy_env:?} build is not \
                 compatible with {network:?} network"
            ),
        }
    }
}

impl From<Network> for common::cli::Network {
    fn from(network: Network) -> Self {
        match network {
            Network::Bitcoin => common::cli::MAINNET_NETWORK,
            Network::Testnet => common::cli::TESTNET_NETWORK,
            Network::Regtest => common::cli::REGTEST_NETWORK,
        }
    }
}
