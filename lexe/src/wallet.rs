use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow, ensure};
use lexe_api::{
    def::{AppBackendApi, AppGatewayApi, AppNodeRunApi},
    models::command::{self, GetUpdatedPayments},
    types::{
        bounded_string::BoundedString,
        payments::{
            ClientPaymentId, PaymentCreatedIndex, PaymentId, PaymentKind,
            PaymentStatus,
        },
    },
};
use lexe_common::{
    api::{
        auth::{
            UserSignupRequestWire, UserSignupRequestWireV1,
            UserSignupRequestWireV2,
        },
        revocable_clients,
        user::NodePkProof,
    },
    ln::{amount::Amount, priority::ConfirmationPriority},
};
use lexe_crypto::rng::SysRng;
use lexe_node_client::client::{GatewayClient, NodeClient};
use lexe_payment_uri::{
    self, Bip321Uri, ClaimMethod, EmailLikeAddress, Lnurl, PaymentMethod,
    PaymentUri,
    bip353::{self, Bip353Client},
    lnurl::LnurlClient,
};
use lexe_std::backoff::Backoff;
use tracing::{debug, info, instrument, warn};

use crate::{
    config::{
        WalletEnvConfig, WalletEnvDbConfig, WalletUserConfig,
        WalletUserDbConfig,
    },
    types::{
        auth::{ClientCredentials, CredentialsRef, RootSeed, UserPk},
        command::{
            AnalyzeRequest, AnalyzeResponse, ClaimableDetails, ClientInfo,
            ClientInfoResponse, CreateClientRequest, CreateClientResponse,
            CreateInvoiceRequest, CreateInvoiceResponse, CreateOfferRequest,
            CreateOfferResponse, GetPaymentRequest, GetPaymentResponse,
            GetUpdatedPaymentsRequest, GetUpdatedPaymentsResponse,
            ListClientsResponse, ListPaymentsResponse, NodeInfo,
            PayInvoiceRequest, PayLnurlRequest, PayOfferRequest, PayRequest,
            PayableDetails, PaymentSyncSummary, RevokeClientRequest,
            UpdateClientRequest, UpdatePersonalNoteRequest,
            WithdrawLnurlRequest,
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
/// with local persistence disabled.
const NO_DB_ERR: &str = "Local persistence is disabled for this wallet";

/// Top-level handle to a Lexe wallet.
pub struct LexeWallet {
    user_config: WalletUserConfig,

    /// Database for persistent storage.
    /// Present for wallets created via `fresh`, `load`, or `load_or_fresh`.
    /// Absent for wallets created via `without_db`.
    db: Option<WalletDb<DiskFs>>,

    gateway_client: GatewayClient,
    node_client: NodeClient,
    bip353_client: Bip353Client,
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
    #[instrument(skip_all, name = "(fresh)")]
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
    #[instrument(skip_all, name = "(load)")]
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
    #[instrument(skip_all, name = "(load-or-fresh)")]
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
    #[instrument(skip_all, name = "(without-db)")]
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

    /// Returns a reference to the [`WalletDb`], or an error if local
    /// persistence is disabled for this wallet.
    fn require_db(&self) -> anyhow::Result<&WalletDb<DiskFs>> {
        self.db.as_ref().ok_or_else(|| anyhow!(NO_DB_ERR))
    }

    /// Returns a reference to the [`PaymentsDb`], or an error if local
    /// persistence is disabled for this wallet.
    fn require_payments_db(&self) -> anyhow::Result<&PaymentsDb<DiskFs>> {
        self.db
            .as_ref()
            .map(WalletDb::payments_db)
            .ok_or_else(|| anyhow!(NO_DB_ERR))
    }

    // --- DB accessors (unstable) --- //

    /// Returns `true` if local persistence is enabled for this wallet.
    pub fn persistence_enabled(&self) -> bool {
        self.db.is_some()
    }

    /// Get a reference to the [`WalletDb`].
    ///
    /// Returns [`None`] if local persistence is disabled for this wallet.
    #[cfg(feature = "unstable")]
    pub fn db(&self) -> Option<&WalletDb<DiskFs>> {
        self.db.as_ref()
    }

    /// Get a reference to the payments database.
    /// This is the primary data source for constructing a payments
    /// list UI.
    ///
    /// Returns [`None`] if local persistence is disabled for this wallet.
    #[cfg(feature = "unstable")]
    pub fn payments_db(&self) -> Option<&PaymentsDb<DiskFs>> {
        self.db.as_ref().map(WalletDb::payments_db)
    }

    // --- Client accessors --- //

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

    // --- Node management --- //

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
    #[instrument(skip_all, name = "(signup)")]
    pub async fn signup(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
    ) -> anyhow::Result<()> {
        let allow_gvfs_access = false;
        let backup_password = None;
        let google_auth_code = None;

        self.signup_inner(
            root_seed,
            partner_pk,
            allow_gvfs_access,
            backup_password,
            google_auth_code,
        )
        .await
    }

    /// [`signup`](Self::signup) but with extra parameters generally only used
    /// by the Lexe App.
    #[cfg(feature = "unstable")]
    #[instrument(skip_all, name = "(signup-custom)")]
    pub async fn signup_custom(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        self.signup_inner(
            root_seed,
            partner_pk,
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
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        // Derive keys and build signup request
        let user_key_pair = root_seed.unstable().derive_user_key_pair();
        let node_key_pair = root_seed.unstable().derive_node_key_pair();
        let node_pk_proof = NodePkProof::sign(&node_key_pair);

        let signup_req = UserSignupRequestWire::V2(UserSignupRequestWireV2 {
            v1: UserSignupRequestWireV1::new(node_pk_proof),
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
    /// node is running the most up-to-date enclave software.
    ///
    /// This fetches the current enclaves from the gateway, computes which
    /// releases need to be provisioned, and provisions them.
    #[instrument(skip_all, name = "(provision)")]
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
    #[instrument(skip_all, name = "(provision-custom)")]
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
            .get_gateway_token()
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
            debug!("Already provisioned to all recent releases");
            return Ok(());
        }

        info!("Provisioning enclaves: {enclaves_to_provision}");

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

    /// Get information about this Lexe node, including balance and channels.
    #[instrument(skip_all, name = "(node-info)")]
    pub async fn node_info(&self) -> anyhow::Result<NodeInfo> {
        self.node_client
            .node_info()
            .await
            .map(NodeInfo::from)
            .context("Failed to get node info")
    }

    // --- Paying and receiving Bitcoin --- //

    /// Get information about a Bitcoin or Lightning payment string, including:
    /// - `payable`: The payable string encoding the payment method.
    /// - `method`: The [`PaymentMethod`] struct encapsulating information
    ///   specific to the payment method (e.g. payment hash, metadata, etc...)
    /// - `amount`/`min_amount`/`max_amount`: The amount constraints requested
    ///   by the receiver.
    ///
    /// See [`PayableDetails`] for all fields.
    ///
    /// The following encodings are supported:
    ///   - BIP 321 URI: `bitcoin:bc1...`
    ///   - Lightning URI: `lightning:ln...`
    ///   - BOLT 11 invoice: `lnbc1...`
    ///   - BOLT 12 offer: `lno1...`
    ///   - Onchain bitcoin address: `bc1...`
    ///   - Human Bitcoin Address: `₿satoshi@lexe.app`
    ///   - Lightning Address: `satoshi@lexe.app`
    ///   - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// Within the encodings, the following payment methods are supported:
    ///   - BOLT 11 invoice
    ///   - BOLT 12 offer
    ///   - Bitcoin address
    ///   - Lightning Address
    ///   - LNURL
    // Sync the encodings list with `pay`
    #[instrument(skip_all, name = "(analyze)")]
    pub async fn analyze(
        &self,
        req: AnalyzeRequest,
    ) -> anyhow::Result<AnalyzeResponse> {
        let network = self.user_config().env_config.wallet_env.network;
        let payment_uri = PaymentUri::parse(&req.payment_string)?;

        let (payment_methods, claim_methods) = lexe_payment_uri::resolve(
            &self.bip353_client,
            &self.lnurl_client,
            network,
            payment_uri,
        )
        .await
        .context("Failed to resolve payment methods.")?;

        let payables = payment_methods
            .into_iter()
            .map(|method| match &method {
                PaymentMethod::Onchain {
                    address,
                    amount,
                    label,
                    message,
                } => {
                    let amount = *amount;
                    let payable = if amount.is_some()
                        || label.is_some()
                        || message.is_some()
                    {
                        let bip_321_uri = Bip321Uri {
                            onchain: vec![address.clone().into_unchecked()],
                            amount,
                            label: label.clone(),
                            message: message.clone(),
                            ..Default::default()
                        };
                        bip_321_uri.to_string()
                    } else {
                        address.to_string()
                    };

                    let description = message.to_owned();
                    let min_amount = None;
                    let max_amount = None;
                    let expires_at = None;

                    PayableDetails {
                        payable,
                        method,
                        description,
                        amount,
                        min_amount,
                        max_amount,
                        expires_at,
                    }
                }
                PaymentMethod::Invoice { invoice } => {
                    let payable = invoice.to_string();
                    let description =
                        invoice.description_str().map(str::to_owned);
                    let amount = invoice.amount();
                    let min_amount = None;
                    let max_amount = None;
                    let expires_at = invoice.expires_at().ok();

                    PayableDetails {
                        payable,
                        method,
                        description,
                        amount,
                        min_amount,
                        max_amount,
                        expires_at,
                    }
                }
                PaymentMethod::Offer {
                    offer,
                    bip321_amount,
                    human_bitcoin_address: _,
                } => {
                    let payable = match bip321_amount {
                        None => offer.to_string(),
                        Some(amount) => {
                            let bip_321_uri = Bip321Uri {
                                amount: Some(*amount),
                                offer: Some(offer.clone()),
                                ..Default::default()
                            };
                            bip_321_uri.to_string()
                        }
                    };

                    let description = offer.description().map(str::to_owned);
                    let amount = *bip321_amount;
                    let min_amount = amount
                        .is_none()
                        .then_some(offer.min_amount())
                        .flatten();
                    let max_amount = None;
                    let expires_at = offer.expires_at();

                    PayableDetails {
                        payable,
                        method,
                        description,
                        amount,
                        min_amount,
                        max_amount,
                        expires_at,
                    }
                }
                PaymentMethod::LnurlPay {
                    pay_request,
                    lnurl,
                    lightning_address: _,
                } => {
                    let payable = lnurl.to_string();
                    let description = pay_request
                        .metadata
                        .long_description
                        .to_owned()
                        .or_else(|| {
                            Some(pay_request.metadata.description.to_owned())
                        });
                    let amount = None;
                    let min_amount = Some(pay_request.min_sendable);
                    let max_amount = Some(pay_request.max_sendable);
                    let expires_at = None;

                    PayableDetails {
                        payable,
                        method,
                        description,
                        amount,
                        min_amount,
                        max_amount,
                        expires_at,
                    }
                }
            })
            .collect();

        let claimables = claim_methods
            .into_iter()
            .map(|method| match &method {
                ClaimMethod::LnurlWithdraw {
                    lnurl,
                    withdraw_request,
                } => {
                    let claimable = lnurl.to_string();
                    let description =
                        Some(withdraw_request.default_description.to_owned());
                    let min_amount = Some(withdraw_request.min_withdrawable);
                    let max_amount = Some(withdraw_request.max_withdrawable);

                    ClaimableDetails {
                        claimable,
                        method,
                        description,
                        min_amount,
                        max_amount,
                    }
                }
            })
            .collect();

        Ok(AnalyzeResponse {
            payables,
            claimables,
        })
    }

    /// Pay any string which encodes a Bitcoin or Lightning payment method.
    ///
    /// If there exist multiple encoded payment methods, one best recommended
    /// payment method will be chosen.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed). Exception: onchain sends return immediately with
    /// the payment still in `Pending` state, since on-chain confirmation takes
    /// ~1 hour.
    ///
    /// For finer control over how to pay, consider first using
    /// [`analyze`](Self::analyze) to resolve the contents of the
    /// payable string, then invoking the specific `pay` function for the
    /// payment method of choice: [`pay_invoice`](Self::pay_invoice),
    /// [`pay_offer`](Self::pay_offer), etc.
    ///
    /// The following encodings are supported:
    ///   - BIP 321 URI: `bitcoin:bc1...`
    ///   - Lightning URI: `lightning:ln...`
    ///   - BOLT 11 invoice: `lnbc1...`
    ///   - BOLT 12 offer: `lno1...`
    ///   - Onchain bitcoin address: `bc1...`
    ///   - Human Bitcoin Address: `₿satoshi@lexe.app`
    ///   - Lightning Address: `satoshi@lexe.app`
    ///   - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// See [`PaymentMethod`] for more details on supported payment methods.
    // Sync the encodings list with `analyze`
    #[instrument(skip_all, name = "(pay)")]
    pub async fn pay(&self, req: PayRequest) -> anyhow::Result<Payment> {
        let PayRequest {
            payable,
            amount,
            message,
            personal_note,
        } = req;

        // Validate note fields against Lexe's limits before any resolution
        let message = message
            .map(BoundedString::new)
            .transpose()
            .context("Invalid message")?;
        let personal_note = personal_note
            .map(BoundedString::new)
            .transpose()
            .context("Invalid personal note")?;

        // Parse the string
        let payment_uri = PaymentUri::parse(&payable)
            .context("Failed to parse payable string")?;

        // Error messages tied to the method can appear unrelated to the og URI;
        // "I analyzed LNURL, why is it talking about BOLT11 invoice?"
        let uri_err_context = match &payment_uri {
            PaymentUri::Bip321Uri(_) => "Failed to pay BIP321 URL",
            PaymentUri::LightningUri(_) => "Failed to pay Lightning URI",
            PaymentUri::Invoice(_) => "Failed to pay invoice",
            PaymentUri::Offer(_) => "Failed to pay offer",
            PaymentUri::Address(_) => "Failed to pay onchain address",
            PaymentUri::EmailLikeAddress(_) =>
                "Failed to pay HBA or Lightning Address",
            PaymentUri::Lnurl(_) => "Failed to pay LNURL",
        };

        // Resolve into best payment method
        let bip353_client = &self.bip353_client;
        let lnurl_client = &self.lnurl_client;
        let network = self.user_config().env_config.wallet_env.network;
        let (maybe_pay_method, _maybe_claim_method) =
            lexe_payment_uri::resolve_best(
                bip353_client,
                lnurl_client,
                network,
                payment_uri,
            )
            .await?;
        let best_pay_method =
            maybe_pay_method.context("No payment method found")?;

        // Create and send the appropriate request
        let index = self
            .pay_inner(best_pay_method, amount, message, personal_note)
            .await
            .context(uri_err_context)?;

        // Lightning payments wait via `wait_for_payment` until they reach a
        // terminal state. Onchain sends take 6 confirmations (~1 hour) to
        // finalize, so we don't wait; we fetch the just-created (pending)
        // payment.
        match index.id {
            PaymentId::OnchainSend(_) => self
                .get_payment(GetPaymentRequest { index })
                .await?
                .payment
                .context("Onchain payment missing right after creation"),
            _ => self.wait_for_payment(index, None).await,
        }
    }

    async fn pay_inner(
        &self,
        best_method: PaymentMethod,
        amount: Option<Amount>,
        message: Option<BoundedString>,
        personal_note: Option<BoundedString>,
    ) -> anyhow::Result<PaymentCreatedIndex> {
        match best_method {
            PaymentMethod::Invoice { invoice } => {
                let fallback_amount = match (invoice.amount(), amount) {
                    (Some(amt), Some(given)) if amt != given =>
                        return Err(anyhow!(
                            "Given amount ({given} sats) doesn't match invoice \
                             amount ({amt} sats)"
                        )),
                    (Some(_), _) => None,
                    (None, Some(amt)) => Some(amt),
                    (None, None) =>
                        return Err(anyhow!(
                            "A payment amount must be provided for amountless \
                             invoices"
                        )),
                };
                if message.is_some() {
                    warn!(
                        "BOLT 11 invoices do not support messages. \
                         The recipient will not see your message."
                    );
                }
                let id = invoice.payment_id();
                let pay_req = command::PayInvoiceRequest {
                    invoice,
                    fallback_amount,
                    message,
                    personal_note,
                    kind: PaymentKind::Invoice,
                };
                let resp = self
                    .node_client
                    .pay_invoice(pay_req)
                    .await
                    .context("Failed to pay invoice")?;
                Ok(PaymentCreatedIndex {
                    created_at: resp.created_at,
                    id,
                })
            }
            PaymentMethod::LnurlPay {
                pay_request,
                lnurl: _,
                lightning_address: _,
            } => {
                let amount = amount.context(
                    "A payment amount must be provided for LNURL payments",
                )?;
                let min_sendable = pay_request.min_sendable;
                let max_sendable = pay_request.max_sendable;
                ensure!(
                    min_sendable <= amount,
                    "Given amount ({amount} sats) should be higher than the \
                     receiver's requested minimum amount: {min_sendable} sats"
                );
                ensure!(
                    amount <= max_sendable,
                    "Given amount ({amount} sats) should be lower than the \
                     receiver's requested maximum amount: {max_sendable} sats"
                );

                // LUD-12: Truncate message to recipient's limit if needed.
                let truncated_comment = match (
                    message.map(BoundedString::into_inner),
                    pay_request.comment_allowed,
                ) {
                    // No message intended; skip.
                    (None, _) => None,
                    // Message intended but recipient doesn't allow comments.
                    // Just log a warning; `pay` should be permissive.
                    (Some(_), None) => {
                        warn!(
                            "Recipient doesn't support LUD-12 comments; \
                             the recipient will not see your message."
                        );
                        None
                    }
                    // Message intended and recipient allows comments; ensure
                    // the comment respects the receiver's specified limit.
                    (Some(mut comment), Some(max_len)) => {
                        let original_len = comment.chars().count();
                        let receiver_limit = usize::from(max_len);

                        lexe_std::string::truncate_chars(
                            &mut comment,
                            receiver_limit,
                        );

                        let truncated = BoundedString::new(comment).expect(
                            "comment was checked above and truncation can \
                             only make it shorter, so the truncated string is \
                             still within bounds.",
                        );

                        if original_len > receiver_limit {
                            warn!(
                                "Message truncated to {receiver_limit} \
                                 character limit specified by recipient: \
                                 \"{truncated}\""
                            );
                        }

                        Some(truncated)
                    }
                };

                let invoice = self
                    .lnurl_client
                    .resolve_pay_request(
                        &pay_request,
                        amount,
                        truncated_comment.as_deref(),
                    )
                    .await?;
                let id = invoice.payment_id();
                let pay_req = command::PayInvoiceRequest {
                    invoice,
                    fallback_amount: None,
                    message: truncated_comment,
                    personal_note,
                    kind: PaymentKind::Invoice,
                };
                let resp = self
                    .node_client
                    .pay_invoice(pay_req)
                    .await
                    .context("Failed to pay invoice")?;
                Ok(PaymentCreatedIndex {
                    created_at: resp.created_at,
                    id,
                })
            }
            PaymentMethod::Offer {
                offer,
                bip321_amount,
                human_bitcoin_address: _,
            } => {
                let amount = match (bip321_amount, amount) {
                    (Some(amt), Some(given)) if amt != given =>
                        return Err(anyhow!(
                            "Given amount ({given} sats) doesn't match bip321 \
                             amount ({amt} sats)"
                        )),
                    (Some(amt), _) | (None, Some(amt)) => amt,
                    (None, None) =>
                        return Err(anyhow!(
                            "A payment amount must be provided for offers \
                             without a bip321-specified amount"
                        )),
                };
                if let Some(min_amount) = offer.min_amount() {
                    ensure!(
                        min_amount <= amount,
                        "Given amount ({amount} sats) should be higher than the \
                         receiver's requested minimum amount: {min_amount} sats"
                    );
                }
                let pay_req = PayOfferRequest {
                    offer,
                    amount,
                    message: message.map(BoundedString::into_inner),
                    personal_note: personal_note.map(BoundedString::into_inner),
                };
                let cid = ClientPaymentId::generate();
                let id = PaymentId::OfferSend(cid);
                let req = pay_req.into_unstable(cid)?;
                let resp = self
                    .node_client
                    .pay_offer(req)
                    .await
                    .context("Failed to pay offer")?;
                Ok(PaymentCreatedIndex {
                    created_at: resp.created_at,
                    id,
                })
            }
            PaymentMethod::Onchain {
                amount: onchain_amount,
                address,
                ..
            } => {
                if message.is_some() {
                    warn!(
                        "On-chain payments do not support messages. \
                         The recipient will not see your message."
                    );
                }
                let amount = match (onchain_amount, amount) {
                    (Some(amt), Some(given)) if amt != given =>
                        return Err(anyhow!(
                            "Given amount ({given} sats) doesn't match bip321 \
                             amount ({amt} sats)"
                        )),
                    (Some(amt), _) | (None, Some(amt)) => amt,
                    (None, None) =>
                        return Err(anyhow!(
                            "A payment amount must be provided for on-chain \
                             methods that don't suggest an amount"
                        )),
                };
                let cid = ClientPaymentId::generate();
                let pay_req = command::PayOnchainRequest {
                    cid,
                    address: address.into_unchecked(),
                    amount,
                    priority: ConfirmationPriority::Normal,
                    personal_note,
                };
                let resp = self
                    .node_client
                    .pay_onchain(pay_req)
                    .await
                    .context("Failed to pay on-chain")?;

                let id = PaymentId::OnchainSend(cid);
                Ok(PaymentCreatedIndex {
                    created_at: resp.created_at,
                    id,
                })
            }
        }
    }

    /// Create a BOLT 11 invoice to receive a Lightning payment.
    #[instrument(skip_all, name = "(create-invoice)")]
    pub async fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> anyhow::Result<CreateInvoiceResponse> {
        let req = command::CreateInvoiceRequest::try_from(req)?;
        let resp = self
            .node_client
            .create_invoice(req)
            .await
            .context("Failed to create invoice")?;

        let index = resp.created_index.context("Node is out of date")?;

        Ok(CreateInvoiceResponse::new(index, resp.invoice))
    }

    /// Pay a BOLT 11 invoice over Lightning.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed).
    #[instrument(skip_all, name = "(pay-invoice)")]
    pub async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> anyhow::Result<Payment> {
        let id = req.invoice.payment_id();
        let req = command::PayInvoiceRequest::try_from(req)?;
        let resp = self
            .node_client
            .pay_invoice(req)
            .await
            .context("Failed to pay invoice")?;

        let index = PaymentCreatedIndex {
            created_at: resp.created_at,
            id,
        };

        self.wait_for_payment(index, None).await
    }

    /// Create a BOLT 12 offer to receive Lightning payments.
    ///
    /// Unlike invoices, offers are reusable: multiple payments can be made to
    /// it, including from multiple payers.
    #[instrument(skip_all, name = "(create-offer)")]
    pub async fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> anyhow::Result<CreateOfferResponse> {
        let req = command::CreateOfferRequest::try_from(req)?;
        let resp = self
            .node_client
            .create_offer(req)
            .await
            .context("Failed to create offer")?;

        Ok(CreateOfferResponse { offer: resp.offer })
    }

    /// Pay a BOLT 12 offer over Lightning.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed).
    #[instrument(skip_all, name = "(pay-offer)")]
    pub async fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> anyhow::Result<Payment> {
        let cid = ClientPaymentId::generate();
        let id = PaymentId::OfferSend(cid);
        let req = req.into_unstable(cid)?;
        let resp = self
            .node_client
            .pay_offer(req)
            .await
            .context("Failed to pay offer")?;

        let index = PaymentCreatedIndex {
            created_at: resp.created_at,
            id,
        };
        self.wait_for_payment(index, None).await
    }

    /// Pay an LNURL or Lightning Address via the `payRequest` flow.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed).
    #[instrument(skip_all, name = "(pay-lnurl)")]
    pub async fn pay_lnurl(
        &self,
        req: PayLnurlRequest,
    ) -> anyhow::Result<Payment> {
        let message = req
            .message
            .map(BoundedString::new)
            .transpose()
            .context("Invalid message")?;
        let personal_note = req
            .personal_note
            .map(BoundedString::new)
            .transpose()
            .context("Invalid personal note")?;

        // Get the pay request
        let pay_request = match (req.lnurl, req.pay_request) {
            (Some(s), None) => {
                // Try to parse as Lightning Address first
                let lnurl = if let Ok(e) = EmailLikeAddress::parse(&s) {
                    if e.bip353_prefix {
                        return Err(anyhow!(
                            "Expected a Lightning Address but found a \
                             ₿-prefixed BIP353 address: {s}"
                        ));
                    }
                    let http_url = e.lightning_address_url;
                    Lnurl::from_http_url(&http_url)?.into_owned()
                } else {
                    Lnurl::parse(&s)?
                };

                self.lnurl_client
                    .get_pay_request(&lnurl)
                    .await
                    .context("Failed to resolve LNURL into pay request")?
            }
            (None, Some(pay_request)) => pay_request,
            _ =>
                return Err(anyhow!(
                    "Exactly one of `pay_request` or `lnurl` must be provided"
                )),
        };

        // Truncate/remove the message based on `comment_allowed`
        let truncated_comment = match (
            message.map(BoundedString::into_inner),
            pay_request.comment_allowed,
        ) {
            (None, _) => None,
            (Some(_), None) => {
                warn!(
                    "Recipient doesn't support LUD-12 comments; the recipient \
                     will not see your message."
                );
                None
            }
            (Some(mut comment), Some(max_len)) => {
                let original_len = comment.chars().count();
                let receiver_limit = usize::from(max_len);

                lexe_std::string::truncate_chars(&mut comment, receiver_limit);

                let truncated = BoundedString::new(comment).expect(
                    "comment was checked above and truncation can \
                     only make it shorter, so the truncated string is \
                     still within bounds.",
                );

                if original_len > receiver_limit {
                    warn!(
                        "Message truncated to {receiver_limit} character limit \
                         specified by recipient: \"{truncated}\""
                    );
                }

                Some(truncated)
            }
        };

        // Make the LNURL callback to get the invoice
        let invoice = self
            .lnurl_client
            .resolve_pay_request(
                &pay_request,
                req.amount,
                truncated_comment.as_deref(),
            )
            .await
            .context("Failed to resolve LNURL pay request")?;

        // Pay invoice
        let id = invoice.payment_id();
        let invoice_req = command::PayInvoiceRequest {
            invoice,
            fallback_amount: None,
            message: truncated_comment,
            personal_note,
            kind: PaymentKind::Invoice,
        };
        let invoice_resp = self
            .node_client
            .pay_invoice(invoice_req)
            .await
            .context("Failed to pay invoice from LNURL pay request")?;

        let index = PaymentCreatedIndex {
            created_at: invoice_resp.created_at,
            id,
        };

        self.wait_for_payment(index, None).await
    }

    /// Withdraw an LNURL via the `withdrawRequest` flow.
    ///
    /// Returns the resulting [`Payment`] once the withdrawal reaches a
    /// terminal state (completed or failed).
    #[instrument(skip_all, name = "(withdraw-lnurl)")]
    pub async fn withdraw_lnurl(
        &self,
        req: WithdrawLnurlRequest,
    ) -> anyhow::Result<Payment> {
        /// The amount of time to wait for the payment after making
        /// the LNURL callback.
        const TIMEOUT: Duration = Duration::from_secs(60);

        // Get the withdraw request
        let withdraw_request = match (req.lnurl, req.withdraw_request) {
            (Some(s), None) => {
                let lnurl = Lnurl::parse(&s)
                    .context("Failed to parse LNURL from string")?;

                self.lnurl_client
                    .get_withdraw_request(&lnurl)
                    .await
                    .context("Failed to resolve LNURL into withdraw request")?
            }
            (None, Some(withdraw_request)) => withdraw_request,
            _ =>
                return Err(anyhow!(
                    "Exactly one of `withdraw_request` or \
                     `lnurl` must be provided"
                )),
        };

        // Create an invoice according to the withdraw request.
        // A `None` amount withdraws the maximum allowed.
        let amount = match req.amount {
            Some(amt) => {
                // Ensure amount is within bounds
                ensure!(
                    withdraw_request.min_withdrawable <= amt,
                    "Given amount ({amt} sats) shouldn't be lower than the \
                     receiver's requested minimum amount: \
                     {} sats",
                    withdraw_request.min_withdrawable
                );
                ensure!(
                    amt <= withdraw_request.max_withdrawable,
                    "Given amount ({amt} sats) shouldn't be higher than the \
                     receiver's requested maximum amount: \
                     {} sats",
                    withdraw_request.max_withdrawable
                );
                amt
            }
            None => withdraw_request.max_withdrawable,
        };

        let invoice_req = CreateInvoiceRequest {
            expiration_secs: None,
            amount: Some(amount),
            description: Some(req.description.unwrap_or_else(|| {
                withdraw_request.default_description.to_owned()
            })),
            personal_note: req.personal_note,
            partner_pk: None,
            partner_prop_fee: None,
            partner_base_fee: None,
        };
        let invoice_resp = self
            .create_invoice(invoice_req)
            .await
            .context("Failed to create invoice for withdrawal request")?;

        // Make the LNURL callback with the invoice
        self.lnurl_client
            .resolve_withdraw_request(&withdraw_request, invoice_resp.invoice)
            .await
            .context("Failed to make LNURL withdraw callback")?;

        // Wait for the payment
        self.wait_for_payment(invoice_resp.index, Some(TIMEOUT))
            .await
            .context("Couldn't receive withdrawal payment")
    }

    // --- Payment information and management --- //

    /// Sync payments from the user node to the local payments cache.
    ///
    /// Returns an error if local persistence is disabled for this wallet.
    #[instrument(skip_all, name = "(sync-payments)")]
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
    /// Returns an error if local persistence is disabled for this wallet.
    ///
    /// [`sync_payments`]: Self::sync_payments
    #[instrument(skip_all, name = "(list-payments)")]
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

    /// Clear all locally cached payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call [`sync_payments`](Self::sync_payments) to re-populate.
    ///
    /// Returns an error if local persistence is disabled for this wallet.
    #[instrument(skip_all, name = "(clear-payments)")]
    pub fn clear_payments(&self) -> anyhow::Result<()> {
        self.require_payments_db()?
            .clear()
            .context("Failed to clear local payments")
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 600 seconds (10 minutes).
    /// Maximum timeout is 86,400 seconds (24 hours).
    #[instrument(skip_all, name = "(wait-for-payment)")]
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

        let initial_wait_ms = 250;
        let max_wait_ms = 4_000;
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

    /// Get information about a payment by its created index.
    #[instrument(skip_all, name = "(get-payment)")]
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

    /// Get a batch of payments in ascending `updated_at` order, starting from
    /// a given `updated_at` index.
    ///
    /// Useful for tailing / syncing payment updates as they occur and merging
    /// them into a local payments store.
    #[instrument(skip_all, name = "(get-updated-payments)")]
    pub async fn get_updated_payments(
        &self,
        req: GetUpdatedPaymentsRequest,
    ) -> anyhow::Result<GetUpdatedPaymentsResponse> {
        let req = GetUpdatedPayments {
            start_index: req.start_index,
            limit: req.limit,
        };
        let resp = self
            .node_client
            .get_updated_payments(req)
            .await
            .context("Failed to get updated payments")?;
        let updated_index = resp.payments.last().map(|p| p.updated_index());
        let payments = resp.payments.into_iter().map(Payment::from).collect();
        Ok(GetUpdatedPaymentsResponse {
            payments,
            updated_index,
        })
    }

    /// Update the personal note on an existing payment.
    /// The note is stored on the user node and is not visible to the
    /// counterparty.
    #[instrument(skip_all, name = "(update-personal-note)")]
    pub async fn update_personal_note(
        &self,
        req: UpdatePersonalNoteRequest,
    ) -> anyhow::Result<()> {
        let req = command::UpdatePersonalNote::try_from(req)?;

        // Update remote store first
        self.node_client
            .update_personal_note(req.clone())
            .await
            .context("Failed to update personal note on user node")?;

        // Success. If persistence is enabled, update the local payments store.
        if let Some(db) = &self.db {
            db.payments_db().update_personal_note(req)?;
        }

        Ok(())
    }

    // --- Client credentials management --- //

    /// List the active clients for this node.
    ///
    /// Revoked and expired clients are not included.
    #[instrument(skip_all, name = "(list-clients)")]
    pub async fn list_clients(&self) -> anyhow::Result<ListClientsResponse> {
        let req = revocable_clients::GetRevocableClients { valid_only: true };
        let clients = self
            .node_client
            .get_revocable_clients(req)
            .await?
            .clients
            .into_iter()
            .map(|(pk, rc)| (pk, ClientInfo::from(rc)))
            .collect();

        Ok(ListClientsResponse { clients })
    }

    /// Create new client credentials for this node.
    ///
    /// WARNING: Anyone with the returned credentials can control this node's
    /// funds. Store them somewhere safe.
    #[instrument(skip_all, name = "(create-client)")]
    pub async fn create_client(
        &self,
        req: CreateClientRequest,
    ) -> anyhow::Result<CreateClientResponse> {
        let req = revocable_clients::CreateRevocableClientRequest::from(req);
        let (rev_client, client_creds) =
            self.node_client.create_client_credentials(req).await?;
        Ok(CreateClientResponse {
            client_pk: client_creds.client_pk,
            client_credentials: ClientCredentials::from_unstable(client_creds),
            created_at: rev_client.created_at,
        })
    }

    /// Update a client's label or expiration.
    #[instrument(skip_all, name = "(update-client)")]
    pub async fn update_client(
        &self,
        req: UpdateClientRequest,
    ) -> anyhow::Result<ClientInfoResponse> {
        let req = revocable_clients::UpdateClientRequest::from(req);
        let client =
            self.node_client.update_revocable_client(req).await?.client;
        Ok(ClientInfoResponse {
            client: ClientInfo::from(client),
        })
    }

    /// Permanently revoke a client, making its credentials invalid for
    /// authentication. This cannot be undone.
    #[instrument(skip_all, name = "(revoke-client)")]
    pub async fn revoke_client(
        &self,
        req: RevokeClientRequest,
    ) -> anyhow::Result<ClientInfoResponse> {
        let req = revocable_clients::UpdateClientRequest {
            pubkey: req.client_pk,
            is_revoked: Some(true),
            label: None,
            expires_at: None,
            scope: None,
        };
        let client =
            self.node_client.update_revocable_client(req).await?.client;
        Ok(ClientInfoResponse {
            client: ClientInfo::from(client),
        })
    }
}
