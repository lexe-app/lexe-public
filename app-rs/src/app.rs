use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use anyhow::Context;
use common::api::auth::{UserAuthenticator, UserSignupRequest};
use common::api::def::{OwnerNodeProvisionApi, UserBackendApi};
use common::api::provision::NodeProvisionRequest;
use common::api::{NodePk, NodePkProof, UserPk};
use common::client::tls::dummy_lexe_ca_cert;
use common::client::NodeClient;
use common::rng::SysRng;
use common::root_seed::RootSeed;
use common::{attest, constants, enclave, Secret};
use secrecy::ExposeSecret;

use crate::bindings::Config;
use crate::secret_store::SecretStore;

#[allow(dead_code)] // TODO(phlip9): remove
pub struct App {
    instance_id: i32,
    secret_store: SecretStore,
    node_client: NodeClient,
}

impl App {
    fn next_instance_id() -> i32 {
        static INSTANCE_ID_COUNTER: AtomicI32 = AtomicI32::new(0);
        INSTANCE_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    #[inline]
    pub fn instance_id(&self) -> i32 {
        self.instance_id
    }

    pub fn test_method(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Load the app state from local storage. Returns `None` if this is the
    /// first run.
    pub async fn load(_config: Config) -> anyhow::Result<Option<Self>> {
        // TODO(phlip9): load from disk
        Ok(None)
    }

    pub async fn recover(
        _config: Config,
        _seed_phrase: String,
    ) -> anyhow::Result<Self> {
        todo!()
    }

    pub async fn signup(_config: Config) -> anyhow::Result<Self> {
        // TODO: need to get the initial enclave measurement from somewhere
        let measurement = enclave::MOCK_MEASUREMENT;
        let gateway_url = "http://phliptop-mbp.attlocal.net:4040".to_owned();
        let use_sgx = false;

        let mut rng = SysRng::new();

        // sample the RootSeed

        let root_seed = RootSeed::from_rng(&mut rng);

        // derive user key and node key

        let user_key_pair = root_seed.derive_user_key_pair();
        let user_pk = UserPk::from(*user_key_pair.public_key());
        let node_key_pair = root_seed.derive_node_key_pair(&mut rng);
        let node_pk = NodePk(node_key_pair.public_key());

        // gen + sign the UserSignupRequest

        let node_pk_proof = NodePkProof::sign(&mut rng, &node_key_pair);
        let signup_req = UserSignupRequest::new(node_pk_proof);
        let (_, signed_signup_req) = user_key_pair
            .sign_struct(&signup_req)
            .expect("Should never fail to serialize UserSignupRequest");

        // build NodeClient

        let enclave_policy = attest::EnclavePolicy {
            // TODO(phlip9): allow_debug should depend on the enclave build
            // setting
            allow_debug: true,
            trusted_mrenclaves: Some(vec![measurement]),
            trusted_mrsigner: None,
        };
        let attest_verifier = attest::ServerCertVerifier {
            expect_dummy_quote: !use_sgx,
            enclave_policy,
        };

        let user_authenticator =
            Arc::new(UserAuthenticator::new(user_key_pair, None));

        let node_client = NodeClient::new(
            &mut rng,
            &root_seed,
            user_authenticator,
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
            instance_id: Self::next_instance_id(),
            secret_store,
            node_client,
        })
    }
}
