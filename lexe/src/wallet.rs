use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow, ensure};
use lexe_api::{
    def::{AppBackendApi, AppNodeRunApi},
    models::command,
    types::payments::{PaymentCreatedIndex, PaymentStatus},
};
use lexe_common::api::{
    auth::{
        UserSignupRequestWire, UserSignupRequestWireV1, UserSignupRequestWireV2,
    },
    user::NodePkProof,
};
use lexe_crypto::rng::SysRng;
use lexe_node_client::client::{GatewayClient, NodeClient};
use lexe_payment_uri::{
    bip353::{self, Bip353Client},
    lnurl::LnurlClient,
};
use lexe_std::backoff::Backoff;
use tracing::info;

use crate::{
    config::{
        WalletEnvConfig, WalletEnvDbConfig, WalletUserConfig,
        WalletUserDbConfig,
    },
    types::{
        auth::{CredentialsRef, RootSeed, UserPk},
        command::{
            CreateInvoiceRequest, CreateInvoiceResponse, GetPaymentRequest,
            GetPaymentResponse, ListPaymentsResponse, NodeInfo,
            PayInvoiceRequest, PayInvoiceResponse, PaymentSyncSummary,
            UpdatePaymentNoteRequest,
        },
        payment::{Order, Payment, PaymentFilter},
    },
    unstable::{
        ffs::DiskFs, payments_db::PaymentsDb, provision, wallet_db::WalletDb,
    },
};

/// Default number of payments per page.
const DEFAULT_LIST_LIMIT: usize = 100;

const WAIT_FOR_PAYMENT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const WAIT_FOR_PAYMENT_MAX_TIMEOUT: Duration =
    Duration::from_secs(24 * 60 * 60);

/// Error message returned when a DB-required method is called on a wallet
/// created without local persistence.
const NO_DB_ERR: &str = "This wallet was created without local persistence";

/// Top-level handle to a Lexe wallet.
pub struct LexeWallet {
    user_config: WalletUserConfig,

    /// Database for persistent storage.
    /// Present for wallets created via `fresh`, `load`, or `load_or_fresh`.
    /// Absent for wallets created via `without_db`.
    db: Option<WalletDb<DiskFs>>,

    gateway_client: GatewayClient,
    node_client: NodeClient,
    #[allow(dead_code)] // TODO(max): Remove
    bip353_client: Bip353Client,
    #[allow(dead_code)] // TODO(max): Remove
    lnurl_client: LnurlClient,
}

// TODO(max): Consider what happens if someone provides *both* a client
// credential and a root seed for the same user. Do we need locks for the dbs?

impl LexeWallet {
    // --- Constructors --- //

    /// Create a fresh [`LexeWallet`], deleting any existing database state for
    /// this user. Data for other users and environments is not affected.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this [`LexeWallet`] is for. Users and environments will not
    /// interfere with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn fresh(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let lexe_data_dir =
            lexe_data_dir.map_or_else(crate::default_lexe_data_dir, Ok)?;
        let env_db_config =
            WalletEnvDbConfig::new(env_config.wallet_env, lexe_data_dir);
        let user_db_config =
            WalletUserDbConfig::from_credentials(credentials, env_db_config)?;

        let db = WalletDb::fresh(user_db_config)
            .context("Failed to create fresh wallet db")?;

        let (
            user_config,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        ) = Self::build_clients(env_config, credentials)?;

        Ok(Self {
            user_config,
            db: Some(db),
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        })
    }

    /// Load an existing [`LexeWallet`] with persistence from `lexe_data_dir`.
    /// Returns [`None`] if no local data exists, in which case you should use
    /// [`fresh`] to create the wallet and local data cache.
    ///
    /// If you are authenticating with [`RootSeed`]s and this returns [`None`],
    /// you should call [`signup`] after creating the wallet if you're not sure
    /// whether the user has been signed up with Lexe.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this [`LexeWallet`] is for. Users and environments will not
    /// interfere with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    ///
    /// [`fresh`]: LexeWallet::fresh
    /// [`signup`]: LexeWallet::signup
    pub fn load(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Option<Self>> {
        let lexe_data_dir =
            lexe_data_dir.map_or_else(crate::default_lexe_data_dir, Ok)?;
        let env_db_config =
            WalletEnvDbConfig::new(env_config.wallet_env, lexe_data_dir);
        let user_db_config =
            WalletUserDbConfig::from_credentials(credentials, env_db_config)?;

        let maybe_db = WalletDb::load(user_db_config)
            .context("Failed to load wallet db")?;
        let db = match maybe_db {
            Some(d) => d,
            None => return Ok(None),
        };

        let (
            user_config,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        ) = Self::build_clients(env_config, credentials)?;

        Ok(Some(Self {
            user_config,
            db: Some(db),
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        }))
    }

    /// Load an existing [`LexeWallet`] with persistence from `lexe_data_dir`,
    /// or create a fresh one if no local data exists. If you are authenticating
    /// with client credentials, this is generally what you want to use.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this [`LexeWallet`] is for. Users and environments will not
    /// interfere with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn load_or_fresh(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let lexe_data_dir =
            lexe_data_dir.map_or_else(crate::default_lexe_data_dir, Ok)?;
        let env_db_config =
            WalletEnvDbConfig::new(env_config.wallet_env, lexe_data_dir);
        let user_db_config =
            WalletUserDbConfig::from_credentials(credentials, env_db_config)?;

        let db = WalletDb::load_or_fresh(user_db_config)
            .context("Failed to load or create wallet db")?;

        let (
            user_config,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        ) = Self::build_clients(env_config, credentials)?;

        Ok(Self {
            user_config,
            db: Some(db),
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        })
    }

    /// Create a [`LexeWallet`] without any persistence. It is recommended to
    /// use [`fresh`] or [`load`] instead, to initialize with persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations ([`sync_payments`], [`list_payments`],
    /// [`clear_payments`]) are not available and will return an error.
    ///
    /// [`fresh`]: LexeWallet::fresh
    /// [`load`]: LexeWallet::load
    /// [`sync_payments`]: LexeWallet::sync_payments
    /// [`list_payments`]: LexeWallet::list_payments
    /// [`clear_payments`]: LexeWallet::clear_payments
    pub fn without_db(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<Self> {
        let (
            user_config,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        ) = Self::build_clients(env_config, credentials)?;

        Ok(Self {
            user_config,
            db: None,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        })
    }

    /// Helper to construct the required clients.
    fn build_clients(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<(
        WalletUserConfig,
        GatewayClient,
        NodeClient,
        Bip353Client,
        LnurlClient,
    )> {
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

        let mut rng = SysRng::new();
        let node_client = NodeClient::new(
            &mut rng,
            env_config.wallet_env.use_sgx,
            env_config.wallet_env.deploy_env,
            gateway_client.clone(),
            credentials.to_unstable(),
        )
        .context("Failed to build NodeClient")?;

        let bip353_client = Bip353Client::new(bip353::GOOGLE_DOH_ENDPOINT)
            .context("Failed to build BIP353 client")?;

        let lnurl_client = LnurlClient::new(env_config.wallet_env.deploy_env)
            .context("Failed to build LNURL client")?;

        Ok((
            user_config,
            gateway_client,
            node_client,
            bip353_client,
            lnurl_client,
        ))
    }

    // --- DB helpers --- //

    /// Returns a reference to the [`WalletDb`], or an error if this wallet
    /// was created without local persistence.
    fn require_db(&self) -> anyhow::Result<&WalletDb<DiskFs>> {
        self.db.as_ref().ok_or_else(|| anyhow!(NO_DB_ERR))
    }

    /// Returns a reference to the [`PaymentsDb`], or an error if this wallet
    /// was created without local persistence.
    fn require_payments_db(&self) -> anyhow::Result<&PaymentsDb<DiskFs>> {
        self.db
            .as_ref()
            .map(WalletDb::payments_db)
            .ok_or_else(|| anyhow!(NO_DB_ERR))
    }

    // --- DB accessors (unstable) --- //

    /// Get a reference to the [`WalletDb`].
    ///
    /// Returns [`None`] if this wallet was created without local persistence.
    #[cfg(feature = "unstable")]
    pub fn db(&self) -> Option<&WalletDb<DiskFs>> {
        self.db.as_ref()
    }

    /// Get a reference to the payments database.
    /// This is the primary data source for constructing a payments
    /// list UI.
    ///
    /// Returns [`None`] if this wallet was created without local
    /// persistence.
    #[cfg(feature = "unstable")]
    pub fn payments_db(&self) -> Option<&PaymentsDb<DiskFs>> {
        self.db.as_ref().map(WalletDb::payments_db)
    }

    // --- DB-required methods --- //

    /// Sync payments from the user node to the local database.
    /// This fetches updated payments from the node and persists them locally.
    ///
    /// Returns an error if this wallet was created without local persistence.
    pub async fn sync_payments(&self) -> anyhow::Result<PaymentSyncSummary> {
        self.require_db()?
            .sync_payments(
                &self.node_client,
                lexe_common::constants::DEFAULT_PAYMENTS_BATCH_SIZE,
            )
            .await
    }

    /// List payments from local storage with cursor-based pagination.
    ///
    /// Defaults to descending order (newest first) with a limit of 100.
    ///
    /// To continue paginating, set `after` to the `next_index` from the
    /// previous response. `after` is an *exclusive* index.
    ///
    /// If needed, use [`sync_payments`] to fetch the latest data from the
    /// node before calling this method.
    ///
    /// Returns an error if this wallet was created without local persistence.
    ///
    /// [`sync_payments`]: Self::sync_payments
    pub fn list_payments(
        &self,
        filter: &PaymentFilter,
        order: Option<Order>,
        limit: Option<usize>,
        after: Option<&PaymentCreatedIndex>,
    ) -> anyhow::Result<ListPaymentsResponse> {
        let order = order.unwrap_or(Order::Desc);
        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let (basics, next_index) = self
            .require_payments_db()?
            .list_payments(filter, order, limit, after);
        let payments = basics.into_iter().map(Payment::from).collect();
        Ok(ListPaymentsResponse {
            payments,
            next_index,
        })
    }

    /// Clear all local payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call [`sync_payments`](Self::sync_payments) to re-populate.
    ///
    /// Returns an error if this wallet was created without local persistence.
    pub fn clear_payments(&self) -> anyhow::Result<()> {
        self.require_payments_db()?
            .clear()
            .context("Failed to clear local payments")
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 10 minutes if not specified.
    /// Maximum timeout is 86,400 seconds (24 hours).
    pub async fn wait_for_payment(
        &self,
        index: PaymentCreatedIndex,
        timeout: Option<Duration>,
    ) -> anyhow::Result<Payment> {
        let timeout = timeout.unwrap_or(WAIT_FOR_PAYMENT_DEFAULT_TIMEOUT);
        let max_secs = WAIT_FOR_PAYMENT_MAX_TIMEOUT.as_secs();
        let timeout_secs = timeout.as_secs();
        ensure!(
            timeout <= WAIT_FOR_PAYMENT_MAX_TIMEOUT,
            "Timeout exceeds max of {max_secs}s (24 hours): {timeout_secs}s",
        );

        let initial_wait_ms = 1_000;
        let max_wait_ms = 5 * 60 * 1_000;
        let start = tokio::time::Instant::now();
        let mut backoff = Backoff::new(initial_wait_ms, max_wait_ms);

        loop {
            // Fetch the latest payment state.
            let payment = if self.db.is_some() {
                // DB-backed path: sync payments and query local DB.
                self.sync_payments().await?;
                self.require_payments_db()?
                    .get_payment_by_created_index(&index)
                    .map(Payment::from)
            } else {
                // No-DB path: poll the node directly.
                self.node_client
                    .get_payment_by_id(command::PaymentIdStruct {
                        id: index.id,
                    })
                    .await
                    .context("Failed to get payment")?
                    .maybe_payment
                    .map(Payment::from)
            };

            if let Some(payment) = payment {
                match payment.status {
                    PaymentStatus::Completed | PaymentStatus::Failed =>
                        return Ok(payment),
                    PaymentStatus::Pending => (), // Continue polling
                }
            }

            ensure!(
                start.elapsed() < timeout,
                "Payment did not complete within {timeout_secs}s timeout",
            );

            tokio::time::sleep(backoff.next().unwrap()).await;
        }
    }

    // --- Shared methods --- //

    /// Registers this user with Lexe, then provisions the node.
    /// This method must be called after the user's [`LexeWallet`] has been
    /// created for the first time, otherwise subsequent requests will fail.
    ///
    /// It is only necessary to call this method once, ever, per user, but it
    /// is also okay to call this method even if the user has already been
    /// signed up; in other words, this method is idempotent.
    ///
    /// After a successful signup, make sure the user's root seed has been
    /// persisted somewhere! Without access to their root seed, your user will
    /// lose their funds forever. If adding Lexe to a broader wallet, a good
    /// strategy is to derive Lexe's [`RootSeed`] from your own root seed.
    ///
    /// - `partner_pk`: Set to your company's [`UserPk`] to earn a share of this
    ///   wallet's fees.
    pub async fn signup(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
    ) -> anyhow::Result<()> {
        let signup_code = None;
        let allow_gvfs_access = false;
        let backup_password = None;
        let google_auth_code = None;

        self.signup_inner(
            root_seed,
            partner_pk,
            signup_code,
            allow_gvfs_access,
            backup_password,
            google_auth_code,
        )
        .await
    }

    /// [`signup`](Self::signup) but with extra parameters generally only used
    /// by the Lexe App.
    #[cfg(feature = "unstable")]
    pub async fn signup_custom(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
        signup_code: Option<String>,
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        self.signup_inner(
            root_seed,
            partner_pk,
            signup_code,
            allow_gvfs_access,
            backup_password,
            google_auth_code,
        )
        .await
    }

    // Inner implementation shared by both stable and unstable APIs.
    #[cfg_attr(not(feature = "unstable"), allow(dead_code))]
    async fn signup_inner(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
        signup_code: Option<String>,
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        // Derive keys and build signup request
        let user_key_pair = root_seed.unstable().derive_user_key_pair();
        let node_key_pair = root_seed.unstable().derive_node_key_pair();
        let node_pk_proof = NodePkProof::sign(&node_key_pair);

        let signup_req = UserSignupRequestWire::V2(UserSignupRequestWireV2 {
            v1: UserSignupRequestWireV1 {
                node_pk_proof,
                signup_code,
            },
            partner: partner_pk.map(UserPk::unstable),
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
            .map(|password| root_seed.password_encrypt(password))
            .transpose()
            .context("Could not encrypt root seed under password")?;

        // Initial provisioning
        let credentials = CredentialsRef::from(root_seed);
        self.provision_inner(
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
    /// This should be called every time the wallet is loaded, to ensure the
    /// user is running the most up-to-date enclave software.
    ///
    /// This fetches the current enclaves from the gateway, computes which
    /// releases need to be provisioned, and provisions them.
    pub async fn provision(
        &self,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<()> {
        self.provision_inner(
            credentials,
            false, // allow_gvfs_access
            None,  // encrypted_seed
            None,  // google_auth_code
        )
        .await
    }

    /// [`provision`](Self::provision) but with extra parameters generally only
    /// used by the Lexe App.
    #[cfg(feature = "unstable")]
    pub async fn provision_custom(
        &self,
        credentials: CredentialsRef<'_>,
        allow_gvfs_access: bool,
        encrypted_seed: Option<Vec<u8>>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        self.provision_inner(
            credentials,
            allow_gvfs_access,
            encrypted_seed,
            google_auth_code,
        )
        .await
    }

    // Inner implementation shared by both stable and unstable APIs.
    #[cfg_attr(not(feature = "unstable"), allow(dead_code))]
    async fn provision_inner(
        &self,
        credentials: CredentialsRef<'_>,
        allow_gvfs_access: bool,
        encrypted_seed: Option<Vec<u8>>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        // Only RootSeed can sign; delegated provisioning not implemented yet.
        let CredentialsRef::RootSeed(_root_seed_ref) = credentials else {
            return Err(anyhow!(
                "Delegated provisioning is not implemented yet"
            ));
        };

        let wallet_env = self.user_config.env_config.wallet_env;

        // Get a bearer token for authentication.
        let token = self
            .node_client
            .request_provision_token()
            .await
            .context("Could not get bearer token")?;

        // Build request with our trusted measurements.
        let req = command::EnclavesToProvisionRequest {
            trusted_measurements: provision::LATEST_TRUSTED_MEASUREMENTS
                .clone(),
        };

        let enclaves_to_provision = self
            .gateway_client
            .enclaves_to_provision(&req, token)
            .await
            .context("Could not fetch enclaves to provision")?;

        // Client-side verification: ensure backend only returned enclaves we
        // trust. Skip in dev mode since measurements are mocked.
        if wallet_env.deploy_env.is_staging_or_prod() {
            let all_trusted =
                enclaves_to_provision.enclaves.iter().all(|enclave| {
                    provision::LATEST_TRUSTED_MEASUREMENTS
                        .contains(&enclave.measurement)
                });
            ensure!(all_trusted, "Backend returned untrusted enclaves:");
        }

        if enclaves_to_provision.enclaves.is_empty() {
            info!("Already provisioned to all recent releases");
            return Ok(());
        }

        info!(
            "Provisioning enclaves: {:?}",
            enclaves_to_provision.enclaves
        );

        match credentials {
            CredentialsRef::RootSeed(root_seed_ref) => {
                let root_seed = provision::clone_root_seed(root_seed_ref);

                provision::provision_all(
                    self.node_client.clone(),
                    enclaves_to_provision.enclaves,
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

    /// Get a reference to the [`GatewayClient`].
    #[cfg(feature = "unstable")]
    pub fn gateway_client(&self) -> &GatewayClient {
        &self.gateway_client
    }

    /// Get a reference to the [`NodeClient`].
    #[cfg(feature = "unstable")]
    pub fn node_client(&self) -> &NodeClient {
        &self.node_client
    }

    /// Get a reference to the [`Bip353Client`].
    #[cfg(feature = "unstable")]
    pub fn bip353_client(&self) -> &Bip353Client {
        &self.bip353_client
    }

    /// Get a reference to the [`LnurlClient`].
    #[cfg(feature = "unstable")]
    pub fn lnurl_client(&self) -> &LnurlClient {
        &self.lnurl_client
    }

    // --- Command API --- //

    /// Get information about this Lexe node, including balance and channels.
    pub async fn node_info(&self) -> anyhow::Result<NodeInfo> {
        self.node_client
            .node_info()
            .await
            .map(NodeInfo::from)
            .context("Failed to get node info")
    }

    /// Create a BOLT 11 invoice to receive a Lightning payment.
    pub async fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> anyhow::Result<CreateInvoiceResponse> {
        let req = req.try_into()?;
        let resp = self
            .node_client
            .create_invoice(req)
            .await
            .context("Failed to create invoice")?;

        let index = resp.created_index.context("Node is out of date")?;

        Ok(CreateInvoiceResponse::new(index, resp.invoice))
    }

    /// Pay a BOLT 11 invoice over Lightning.
    pub async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> anyhow::Result<PayInvoiceResponse> {
        let id = req.invoice.payment_id();
        let req: command::PayInvoiceRequest = req.try_into()?;
        let resp = self
            .node_client
            .pay_invoice(req)
            .await
            .context("Failed to pay invoice")?;

        let index = PaymentCreatedIndex {
            created_at: resp.created_at,
            id,
        };

        Ok(PayInvoiceResponse {
            index,
            created_at: resp.created_at,
        })
    }

    /// Get information about a payment by its created index.
    pub async fn get_payment(
        &self,
        req: GetPaymentRequest,
    ) -> anyhow::Result<GetPaymentResponse> {
        let id = req.index.id;
        let payment = self
            .node_client
            .get_payment_by_id(command::PaymentIdStruct { id })
            .await
            .context("Failed to get payment")?
            .maybe_payment
            .map(Into::into);

        Ok(GetPaymentResponse { payment })
    }

    /// Update the personal note on an existing payment.
    /// The note is stored on the user node and is not visible to the
    /// counterparty.
    pub async fn update_payment_note(
        &self,
        req: UpdatePaymentNoteRequest,
    ) -> anyhow::Result<()> {
        let req: command::UpdatePaymentNote = req.try_into()?;

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
