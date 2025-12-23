use std::{marker::PhantomData, path::PathBuf};

use anyhow::{Context, anyhow};
use common::{
    api::{
        auth::{
            UserSignupRequestWire, UserSignupRequestWireV1,
            UserSignupRequestWireV2,
        },
        user::{NodePkProof, UserPk},
    },
    rng::Crng,
    root_seed::RootSeed,
};
use lexe_api::{
    def::{AppBackendApi, AppGatewayApi, AppNodeRunApi},
    models::command::{
        LxPaymentIdStruct, PayInvoiceRequest, UpdatePaymentNote,
    },
    types::payments::PaymentCreatedIndex,
};
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::CredentialsRef,
};
use payment_uri::{
    bip353::{self, Bip353Client},
    lnurl::LnurlClient,
};
use sdk_core::models::{
    SdkCreateInvoiceRequest, SdkCreateInvoiceResponse, SdkGetPaymentRequest,
    SdkGetPaymentResponse, SdkNodeInfo, SdkPayInvoiceRequest,
    SdkPayInvoiceResponse,
};
use tracing::info;

use crate::{
    config::{
        WalletEnvConfig, WalletEnvDbConfig, WalletUserConfig,
        WalletUserDbConfig,
    },
    payments_db::PaymentsDb,
    unstable::{
        ffs::DiskFs, provision, provision_history::ProvisionHistory,
        wallet_db::WalletDb,
    },
};

/// Type state indicating the wallet has persistence enabled.
pub struct WithDb;
/// Type state indicating the wallet has no persistence.
pub struct WithoutDb;

/// Top-level handle to a Lexe wallet.
///
/// Exposes simple and ~stable APIs for easy management of a Lexe wallet.
pub struct LexeWallet<Db> {
    user_config: WalletUserConfig,

    /// Database for persistent storage
    /// Present iff `Db` = `WithDb`.
    db: Option<WalletDb<DiskFs>>,

    gateway_client: GatewayClient,
    node_client: NodeClient,
    bip353_client: Bip353Client,
    lnurl_client: LnurlClient,

    _marker: PhantomData<Db>,
}

// TODO(max): Move `enclaves_to_provision` calculation logic to the backend.
// Add AppBackendApi::enclaves_to_provision() which takes the client's latest
// three trusted measurements and returns which ones to provision. This way, SDK
// clients can provision statelessly; no need to persist provision history.
// TODO(max): Consider what happens if someone provides *both* a client
// credential and a root seed for the same user. Do we need locks for the dbs?

impl LexeWallet<WithDb> {
    /// Create a fresh [`LexeWallet`], deleting any existing database state for
    /// this user. Data for other users and environments is not affected.
    ///
    /// It is recommended to always pass the same `lexe_data_dir` to this
    /// constructor, regardless of which environment we're in (dev/staging/prod)
    /// and which user this [`LexeWallet`] is for. Users and environments will
    /// not interfere with each other as all data is namespaced internally.
    pub fn fresh(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let fresh = true;
        Self::new(rng, env_config, credentials, Some(lexe_data_dir), fresh)
    }

    /// Load an existing [`LexeWallet`] with persistence from `lexe_data_dir`,
    /// or creates the wallet if it didn't exist.
    ///
    /// It is recommended to always pass the same `lexe_data_dir` to this
    /// constructor, regardless of which environment we're in (dev/staging/prod)
    /// and which user this [`LexeWallet`] is for. Users and environments will
    /// not interfere with each other as all data is namespaced internally.
    pub fn load(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let fresh = false;
        Self::new(rng, env_config, credentials, Some(lexe_data_dir), fresh)
    }

    /// Get a reference to the [`WalletDb`].
    #[cfg(feature = "unstable")]
    pub fn db(&self) -> &WalletDb<DiskFs> {
        self.db.as_ref().expect("WithDb always has db")
    }

    /// Get a reference to the [`PaymentsDb`].
    /// This is the primary data source for constructing a payments list UI.
    pub fn payments_db(&self) -> &PaymentsDb<DiskFs> {
        self.db
            .as_ref()
            .expect("WithDb always has db")
            .payments_db()
    }

    /// Sync payments from the user node to the local database.
    /// This fetches updated payments from the node and persists them locally.
    ///
    /// Only one sync can run at a time.
    /// Errors if another sync is already in progress.
    pub async fn sync_payments(
        &self,
    ) -> anyhow::Result<crate::payments_db::PaymentSyncSummary> {
        self.db
            .as_ref()
            .expect("WithDb always has db")
            .sync_payments(
                &self.node_client,
                common::constants::DEFAULT_PAYMENTS_BATCH_SIZE,
            )
            .await
    }
}

impl LexeWallet<WithoutDb> {
    /// Create a [`LexeWallet`] without any persistence. It is recommended to
    /// use [`fresh`] or [`load`] instead, to initialize with persistence.
    ///
    /// [`fresh`]: LexeWallet::fresh
    /// [`load`]: LexeWallet::load
    pub fn without_db(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<Self> {
        let fresh = false;
        let lexe_data_dir = None;
        Self::new(rng, env_config, credentials, lexe_data_dir, fresh)
    }
}

impl<D> LexeWallet<D> {
    // Internal constructor.
    fn new(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
        fresh: bool,
    ) -> anyhow::Result<Self> {
        let user_pk = credentials.user_pk().context(
            "Client credentials are out of date. \
             Please create a new one from within the Lexe wallet app.",
        )?;

        let user_config = WalletUserConfig {
            user_pk,
            env_config: env_config.clone(),
        };

        let gateway_client = GatewayClient::new(
            env_config.wallet_env.deploy_env,
            env_config.gateway_url.clone(),
            env_config.user_agent.clone(),
        )
        .context("Failed to build GatewayClient")?;

        let node_client = NodeClient::new(
            rng,
            env_config.wallet_env.use_sgx,
            env_config.wallet_env.deploy_env,
            gateway_client.clone(),
            credentials,
        )
        .context("Failed to build NodeClient")?;

        let bip353_client = Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT)
            .context("Failed to build BIP353 client")?;

        let lnurl_client = LnurlClient::new(env_config.wallet_env.deploy_env)
            .context("Failed to build LNURL client")?;

        // Init the `WalletDb` if a lexe_data_dir was provided.
        let db = match lexe_data_dir {
            Some(lexe_data_dir) => {
                let env_db_config = WalletEnvDbConfig::new(
                    env_config.wallet_env,
                    lexe_data_dir,
                );
                let user_db_config =
                    WalletUserDbConfig::new(user_pk, env_db_config);

                let db = if fresh {
                    WalletDb::fresh(user_db_config)
                } else {
                    WalletDb::load(user_db_config)
                }
                .context("Failed to init wallet db")?;

                Some(db)
            }
            None => None,
        };

        Ok(Self {
            user_config,

            db,

            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,

            _marker: PhantomData,
        })
    }

    /// Registers this user with the Lexe backend, then provisions the node.
    ///
    /// This function should be called after the user's [`LexeWallet`] has been
    /// created for the very first time. It only needs to be called once, ever.
    ///
    /// After a successful signup, make sure the user's root seed has been
    /// persisted somewhere! Without access to their root seed, your user will
    /// lose their funds forever. If adding Lexe to a broader wallet, a good
    /// strategy is to derive Lexe's [`RootSeed`] from your own root seed.
    ///
    /// - `partner`: SDK users should set this to the [`UserPk`] of their
    ///   company account. In the future, you may receive a share of the fees
    ///   generated by users that you sign up to Lexe.
    /// - `signup_code`: SDK users should generally set this to `None`.
    /// - `allow_gvfs_access`: SDK users should generally set this to `false`.
    /// - `backup_password`: SDK users should generally set this to `None`.
    /// - `google_auth_code`: SDK users should generally set this to `None`.
    ///
    /// [`fresh()`]: LexeWallet::fresh
    /// [`without_db()`]: LexeWallet::without_db
    pub async fn signup_and_provision(
        &self,
        rng: &mut impl Crng,
        root_seed: &RootSeed,
        partner: Option<UserPk>,
        signup_code: Option<String>,
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        // Derive keys and build signup request
        let user_key_pair = root_seed.derive_user_key_pair();
        let node_key_pair = root_seed.derive_node_key_pair(rng);
        let node_pk_proof = NodePkProof::sign(rng, &node_key_pair);

        let signup_req = UserSignupRequestWire::V2(UserSignupRequestWireV2 {
            v1: UserSignupRequestWireV1 {
                node_pk_proof,
                signup_code,
            },
            partner,
        });
        let signed_signup_req = user_key_pair
            .sign_struct(&signup_req)
            .map(|(_buf, signed)| signed)
            .expect("Should never fail to serialize UserSignupRequestWire");

        // Register with backend
        self.gateway_client
            .signup_v2(&signed_signup_req)
            .await
            .context("Failed to signup user")?;

        // Encrypt seed if backup password provided.
        // NOTE: This is very slow; 600K HMAC iterations!
        let encrypted_seed = backup_password
            .map(|password| root_seed.password_encrypt(rng, password))
            .transpose()
            .context("Could not encrypt root seed under password")?;

        // Initial provisioning
        let credentials = CredentialsRef::from(root_seed);
        self.ensure_provisioned(
            credentials,
            allow_gvfs_access,
            encrypted_seed,
            google_auth_code,
        )
        .await
        .context("Initial provision failed")?;

        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    /// This should be called every time we load the wallet.
    ///
    /// This fetches the current enclaves from the gateway, computes which
    /// releases need to be provisioned, and provisions them.
    ///
    /// - `allow_gvfs_access`: SDK users should generally set this to `false`.
    ///   See [`NodeProvisionRequest::allow_gvfs_access`][aga] for details.
    /// - `encrypted_seed`: SDK users should generally set this to `None`. See
    ///   [`NodeProvisionRequest::encrypted_seed`][es] for details.
    /// - `google_auth_code`: SDK users should generally set this to `None`. See
    ///   [`NodeProvisionRequest::google_auth_code`][gac] for details.
    ///
    /// [aga]: common::api::provision::NodeProvisionRequest::allow_gvfs_access
    /// [es]: common::api::provision::NodeProvisionRequest::encrypted_seed
    /// [gac]: common::api::provision::NodeProvisionRequest::google_auth_code
    pub async fn ensure_provisioned(
        &self,
        credentials: CredentialsRef<'_>,
        allow_gvfs_access: bool,
        encrypted_seed: Option<Vec<u8>>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        // Early return: delegated provisioning not implemented yet
        // TODO(max): Remove
        if matches!(credentials, CredentialsRef::ClientCredentials(_)) {
            return Err(anyhow!(
                "Delegated provisioning is not implemented yet"
            ));
        }

        // Fetch the current enclaves
        let current_enclaves = self
            .gateway_client
            .current_enclaves()
            .await
            .context("Could not fetch current enclaves")?;

        let wallet_env = self.user_config.env_config.wallet_env;
        let deploy_env = wallet_env.deploy_env;

        // Compute the set of enclaves to provision.
        // If persistence is enabled, provision all recent trusted releases
        // that haven't been provisioned yet, according to our history.
        // Otherwise, provision all recent trusted releases every time.
        // TODO(max): Compute `enclaves_to_provision` server-side instead.
        let (enclaves_to_provision, provision_ffs, provision_history) =
            match &self.db {
                Some(db) => {
                    // With persistence: compute from provision history
                    let provision_history = db.provision_history().clone();
                    let enclaves_to_provision = {
                        let locked_history = provision_history.lock().unwrap();
                        locked_history
                            .enclaves_to_provision(deploy_env, current_enclaves)
                    };

                    (
                        enclaves_to_provision,
                        Some(db.provision_ffs().clone()),
                        Some(provision_history),
                    )
                }
                None => {
                    // Without persistence: use empty history, so we provision
                    // all trusted current enclaves every time.
                    let enclaves_to_provision = ProvisionHistory::new()
                        .enclaves_to_provision(deploy_env, current_enclaves);
                    let prov_ffs = None;
                    let prov_history = None;

                    (enclaves_to_provision, prov_ffs, prov_history)
                }
            };

        if enclaves_to_provision.is_empty() {
            info!("Already provisioned to all recent releases");
            return Ok(());
        }

        info!("Provisioning enclaves: {enclaves_to_provision:?}");
        match credentials {
            CredentialsRef::RootSeed(root_seed_ref) => {
                let root_seed = provision::clone_root_seed(root_seed_ref);

                provision::provision_all(
                    self.node_client.clone(),
                    provision_ffs,
                    provision_history,
                    enclaves_to_provision,
                    root_seed,
                    wallet_env,
                    google_auth_code,
                    allow_gvfs_access,
                    encrypted_seed,
                )
                .await
                .context("Root seed provision_all failed")?;
            }
            // TODO(max): Implement delegated provisioning
            CredentialsRef::ClientCredentials(_) =>
                return Err(anyhow!(
                    "Delegated provisioning is not implemented yet"
                )),
        }

        Ok(())
    }

    /// Get a reference to the user's wallet configuration.
    pub fn user_config(&self) -> &WalletUserConfig {
        &self.user_config
    }

    /// Get a reference to the inner [`GatewayClient`].
    #[cfg(feature = "unstable")]
    pub fn gateway_client(&self) -> &GatewayClient {
        &self.gateway_client
    }

    /// Get a reference to the inner [`NodeClient`].
    #[cfg(feature = "unstable")]
    pub fn node_client(&self) -> &NodeClient {
        &self.node_client
    }

    /// Get a reference to the inner [`Bip353Client`].
    #[cfg(feature = "unstable")]
    pub fn bip353_client(&self) -> &Bip353Client {
        &self.bip353_client
    }

    /// Get a reference to the inner [`LnurlClient`].
    #[cfg(feature = "unstable")]
    pub fn lnurl_client(&self) -> &LnurlClient {
        &self.lnurl_client
    }

    // --- Command API --- //

    /// Get information about this Lexe node.
    pub async fn node_info(&self) -> anyhow::Result<SdkNodeInfo> {
        self.node_client
            .node_info()
            .await
            .map(SdkNodeInfo::from)
            .context("Failed to get node info")
    }

    /// Create a BOLT 11 invoice.
    pub async fn create_invoice(
        &self,
        req: SdkCreateInvoiceRequest,
    ) -> anyhow::Result<SdkCreateInvoiceResponse> {
        let resp = self
            .node_client
            .create_invoice(req.into())
            .await
            .context("Failed to create invoice")?;

        let index = resp.created_index.context("Node is out of date")?;

        Ok(SdkCreateInvoiceResponse::new(index, resp.invoice))
    }

    /// Pay a BOLT 11 invoice.
    pub async fn pay_invoice(
        &self,
        req: SdkPayInvoiceRequest,
    ) -> anyhow::Result<SdkPayInvoiceResponse> {
        let id = req.invoice.payment_id();
        let resp = self
            .node_client
            .pay_invoice(PayInvoiceRequest::from(req))
            .await
            .context("Failed to pay invoice")?;

        let index = PaymentCreatedIndex {
            created_at: resp.created_at,
            id,
        };

        Ok(SdkPayInvoiceResponse {
            index,
            created_at: resp.created_at,
        })
    }

    /// Get information about a payment by its index.
    pub async fn get_payment(
        &self,
        req: SdkGetPaymentRequest,
    ) -> anyhow::Result<SdkGetPaymentResponse> {
        let id = req.index.id;
        let payment = self
            .node_client
            .get_payment_by_id(LxPaymentIdStruct { id })
            .await
            .context("Failed to get payment")?
            .maybe_payment
            .map(Into::into);

        Ok(SdkGetPaymentResponse { payment })
    }

    /// Update the note on an existing payment.
    pub async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        // Update remote store first
        self.node_client
            .update_payment_note(req.clone())
            .await
            .context("Failed to update payment note on user node")?;

        // Success. If persistence is enabled, update the local payments store.
        if let Some(db) = &self.db {
            db.payments_db().update_payment_note(req)?;
        }

        Ok(())
    }
}
