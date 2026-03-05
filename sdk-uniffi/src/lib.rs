//! Lexe SDK foreign language bindings.
//!
//! This crate is the [UniFFI] base for generating Lexe SDK bindings in
//! languages like Python, Javascript, Swift, and Kotlin.
//!
//! For Rust projects, use the [`lexe`] crate directly.
//!
//! [UniFFI]: https://mozilla.github.io/uniffi-rs/

use std::{fmt, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use common::{
    api::user::UserPk,
    env::DeployEnv as DeployEnvRs,
    ln::{
        amount::Amount as AmountRs, network::LxNetwork as LxNetworkRs,
        priority::ConfirmationPriority as ConfirmationPriorityRs,
    },
    root_seed::RootSeed as RootSeedRs,
};
use lexe::{
    blocking_wallet::BlockingLexeWallet as BlockingLexeWalletRs,
    config::WalletEnvConfig as WalletEnvConfigRs,
    types::{
        command::{
            CreateInvoiceRequest as CreateInvoiceRequestRs,
            CreateInvoiceResponse as CreateInvoiceResponseRs,
            GetPaymentRequest as GetPaymentRequestRs, NodeInfo as NodeInfoRs,
            PayInvoiceRequest as PayInvoiceRequestRs,
            PayInvoiceResponse as PayInvoiceResponseRs,
            SdkUpdatePaymentNoteRequest as SdkUpdatePaymentNoteRequestRs,
        },
        payment::Payment as PaymentRs,
    },
    wallet::LexeWallet as LexeWalletRs,
};
use lexe_api_core::{
    error::GatewayApiError as GatewayApiErrorRs,
    types::{
        invoice::LxInvoice as LxInvoiceRs,
        payments::{
            PaymentCreatedIndex as PaymentCreatedIndexRs,
            PaymentDirection as PaymentDirectionRs,
            PaymentKind as PaymentKindRs, PaymentRail as PaymentRailRs,
            PaymentStatus as PaymentStatusRs,
        },
    },
};
use node_client::credentials::{
    ClientCredentials as ClientCredentialsRs, CredentialsRef,
};
use secrecy::{ExposeSecret, Zeroize};

uniffi::setup_scaffolding!("lexe");

// ================== //
// --- Global API --- //
// ================== //

/// Returns the default Lexe data directory (`~/.lexe`).
#[uniffi::export]
pub fn default_lexe_data_dir() -> FfiResult<String> {
    lexe::default_lexe_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(Into::into)
}

/// Initialize the Lexe logger with the given default log level.
///
/// Call this once at startup to enable logging. Valid levels are:
/// `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`.
#[uniffi::export]
pub fn init_logger(default_level: String) {
    lexe::init_logger(&default_level);
}

// ===================== //
// --- FFI Internals --- //
// ===================== //

/// Error type returned by SDK methods.
#[derive(Debug, uniffi::Object)]
#[uniffi::export(Display)]
pub struct FfiError {
    message: String,
}

#[uniffi::export]
impl FfiError {
    /// Get the error message.
    pub fn message(&self) -> String {
        self.message.clone()
    }
}

impl std::error::Error for FfiError {}

impl From<anyhow::Error> for FfiError {
    fn from(err: anyhow::Error) -> Self {
        Self {
            message: format!("{err:#}"),
        }
    }
}

impl From<GatewayApiErrorRs> for FfiError {
    fn from(err: GatewayApiErrorRs) -> Self {
        Self {
            message: format!("{err:#}"),
        }
    }
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub type FfiResult<T> = std::result::Result<T, FfiError>;

/// Error type for seedphrase file operations.
///
/// Returned by [`RootSeed::read_from_path`], [`RootSeed::write_to_path`],
/// [`WalletEnvConfig::read_seed`], and [`WalletEnvConfig::write_seed`].
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SeedFileError {
    /// The seedphrase file was not found at the given path.
    #[error("Seedphrase file not found: {path}")]
    NotFound { path: String },
    /// The seedphrase file could not be parsed (e.g. invalid mnemonic).
    #[error("Failed to parse seedphrase: {message}")]
    ParseError { message: String },
    /// A seedphrase file already exists at the given path.
    #[error("Seedphrase file already exists: {path}")]
    AlreadyExists { path: String },
    /// An I/O error occurred during the file operation.
    #[error("I/O error: {message}")]
    IoError { message: String },
}

/// Error type for wallet loading operations.
///
/// Returned by [`AsyncLexeWallet::load`] and [`BlockingLexeWallet::load`].
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum LoadWalletError {
    /// No local wallet data exists for this user and environment.
    #[error("No local wallet data found")]
    NotFound,
    /// Failed to load the wallet (e.g. corrupted data, invalid seed).
    #[error("Failed to load wallet: {message}")]
    LoadFailed { message: String },
}

// ===================== //
// --- Configuration --- //
// ===================== //

/// Deployment environment for a wallet.
#[derive(Clone, uniffi::Enum)]
pub enum DeployEnv {
    /// Development environment.
    Dev,
    /// Staging environment.
    Staging,
    /// Production environment.
    Prod,
}

impl From<DeployEnv> for DeployEnvRs {
    fn from(env: DeployEnv) -> Self {
        match env {
            DeployEnv::Dev => Self::Dev,
            DeployEnv::Staging => Self::Staging,
            DeployEnv::Prod => Self::Prod,
        }
    }
}

/// Bitcoin network to use.
#[derive(Clone, uniffi::Enum)]
pub enum LxNetwork {
    /// Bitcoin mainnet.
    Mainnet,
    /// Bitcoin testnet3.
    Testnet3,
    /// Bitcoin testnet4.
    Testnet4,
    /// Bitcoin signet.
    Signet,
    /// Bitcoin regtest.
    Regtest,
}

impl From<LxNetwork> for LxNetworkRs {
    fn from(network: LxNetwork) -> Self {
        match network {
            LxNetwork::Mainnet => Self::Mainnet,
            LxNetwork::Testnet3 => Self::Testnet3,
            LxNetwork::Testnet4 => Self::Testnet4,
            LxNetwork::Signet => Self::Signet,
            LxNetwork::Regtest => Self::Regtest,
        }
    }
}

impl From<LxNetworkRs> for LxNetwork {
    fn from(network: LxNetworkRs) -> Self {
        match network {
            LxNetworkRs::Mainnet => Self::Mainnet,
            LxNetworkRs::Testnet3 => Self::Testnet3,
            LxNetworkRs::Testnet4 => Self::Testnet4,
            LxNetworkRs::Signet => Self::Signet,
            LxNetworkRs::Regtest => Self::Regtest,
        }
    }
}

/// Configuration for a wallet environment.
#[derive(uniffi::Object)]
pub struct WalletEnvConfig {
    deploy_env: DeployEnv,
    network: LxNetwork,
    use_sgx: bool,
    gateway_url: Option<String>,
}

#[uniffi::export]
impl WalletEnvConfig {
    /// Create config for Bitcoin mainnet.
    #[uniffi::constructor]
    pub fn mainnet() -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Prod,
            network: LxNetwork::Mainnet,
            use_sgx: true,
            gateway_url: None,
        })
    }

    /// Create config for Bitcoin testnet3.
    #[uniffi::constructor]
    pub fn testnet3() -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Staging,
            network: LxNetwork::Testnet3,
            use_sgx: true,
            gateway_url: None,
        })
    }

    /// Create config for local development (regtest).
    #[uniffi::constructor(default(use_sgx = false, gateway_url = None))]
    pub fn regtest(use_sgx: bool, gateway_url: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx,
            gateway_url,
        })
    }

    /// Get the configured deployment environment.
    pub fn deploy_env(&self) -> DeployEnv {
        self.deploy_env.clone()
    }

    /// Get the configured Bitcoin network.
    pub fn network(&self) -> LxNetwork {
        self.network.clone()
    }

    /// Whether SGX is enabled for this config.
    pub fn use_sgx(&self) -> bool {
        self.use_sgx
    }

    /// Get the gateway URL for this environment.
    /// For dev, returns the configured override if present.
    /// For staging/prod, returns the canonical deploy-environment URL.
    pub fn gateway_url(&self) -> Option<String> {
        match self.deploy_env {
            DeployEnv::Dev => self.gateway_url.clone(),
            DeployEnv::Staging =>
                Some(DeployEnvRs::Staging.gateway_url(None).into_owned()),
            DeployEnv::Prod =>
                Some(DeployEnvRs::Prod.gateway_url(None).into_owned()),
        }
    }

    /// Returns the path to the seedphrase file for this environment.
    ///
    /// - Mainnet: `<lexe_data_dir>/seedphrase.txt`
    /// - Other environments: `<lexe_data_dir>/seedphrase.<env>.txt`
    pub fn seedphrase_path(&self, lexe_data_dir: String) -> String {
        self.to_rs()
            .seedphrase_path(lexe_data_dir.as_ref())
            .to_string_lossy()
            .into_owned()
    }

    /// Reads a root seed from `~/.lexe/seedphrase[.env].txt`.
    ///
    /// Raises [`SeedFileError::NotFound`] if the file doesn't exist.
    pub fn read_seed(&self) -> Result<Arc<RootSeed>, SeedFileError> {
        let rs = self.to_rs();
        match rs.read_seed() {
            Ok(Some(inner)) => Ok(Arc::new(RootSeed { inner })),
            Ok(None) => {
                let data_dir = default_lexe_data_dir().unwrap_or_default();
                let path = self.seedphrase_path(data_dir);
                Err(SeedFileError::NotFound { path })
            }
            Err(e) => Err(SeedFileError::ParseError {
                message: format!("{e:#}"),
            }),
        }
    }

    /// Writes a root seed's mnemonic to `~/.lexe/seedphrase[.env].txt`.
    ///
    /// Creates parent directories if needed. Returns an error if the file
    /// already exists.
    pub fn write_seed(
        &self,
        root_seed: Arc<RootSeed>,
    ) -> Result<(), SeedFileError> {
        self.to_rs().write_seed(root_seed.as_rs()).map_err(|e| {
            // Check if the root cause is an "already exists" IO error.
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::AlreadyExists
                {
                    let data_dir = default_lexe_data_dir().unwrap_or_default();
                    let path = self.seedphrase_path(data_dir);
                    return SeedFileError::AlreadyExists { path };
                }
            }
            SeedFileError::IoError {
                message: format!("{e:#}"),
            }
        })
    }
}

impl WalletEnvConfig {
    // TODO(max): Could all of these to_rs be `From` impls?
    fn to_rs(&self) -> WalletEnvConfigRs {
        match self.deploy_env {
            DeployEnv::Prod => WalletEnvConfigRs::mainnet(),
            DeployEnv::Staging => WalletEnvConfigRs::testnet3(),
            DeployEnv::Dev => WalletEnvConfigRs::regtest(
                self.use_sgx,
                self.gateway_url.clone(),
            ),
        }
    }
}

// =================== //
// --- Credentials --- //
// =================== //

/// The secret root seed for deriving all user keys and credentials.
#[derive(uniffi::Object)]
pub struct RootSeed {
    inner: RootSeedRs,
}

#[uniffi::export]
impl RootSeed {
    /// Generate a new random root seed.
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self {
            inner: RootSeedRs::generate(),
        })
    }

    /// Create a new root seed from raw bytes.
    ///
    /// The seed must be exactly 32 bytes.
    #[uniffi::constructor]
    pub fn new(mut seed_bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        let inner = RootSeedRs::try_from(seed_bytes.as_slice())?;
        seed_bytes.zeroize();
        Ok(Arc::new(Self { inner }))
    }

    /// Reads a root seed from a seedphrase file containing a BIP39 mnemonic.
    ///
    /// Raises [`SeedFileError::NotFound`] if the file doesn't exist,
    /// or [`SeedFileError::ParseError`] if the file can't be parsed.
    #[uniffi::constructor]
    pub fn read_from_path(path: String) -> Result<Arc<Self>, SeedFileError> {
        match RootSeedRs::read_from_path(path.as_ref()) {
            Ok(Some(inner)) => Ok(Arc::new(Self { inner })),
            Ok(None) => Err(SeedFileError::NotFound { path }),
            Err(e) => Err(SeedFileError::ParseError {
                message: format!("{e:#}"),
            }),
        }
    }

    /// Get the 32-byte root seed.
    pub fn seed_bytes(&self) -> Vec<u8> {
        self.inner.expose_secret().to_vec()
    }

    /// Writes this root seed's mnemonic to a seedphrase file.
    ///
    /// Creates parent directories if needed. Raises
    /// [`SeedFileError::AlreadyExists`] if the file already exists.
    pub fn write_to_path(&self, path: String) -> Result<(), SeedFileError> {
        self.as_rs().write_to_path(path.as_ref()).map_err(|e| {
            // Check if the root cause is an "already exists" IO error.
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::AlreadyExists
                {
                    return SeedFileError::AlreadyExists { path: path.clone() };
                }
            }
            SeedFileError::IoError {
                message: format!("{e:#}"),
            }
        })
    }
}

impl RootSeed {
    fn as_rs(&self) -> &RootSeedRs {
        &self.inner
    }
}

/// Client credentials for authenticating with Lexe.
#[derive(Clone, uniffi::Record)]
pub struct ClientCredentials {
    /// Base64-encoded credentials blob.
    pub credentials_base64: String,
}

impl ClientCredentials {
    // TODO(mpch): Remove once credentials auth flow is implemented
    #[allow(dead_code)]
    fn to_rs(&self) -> FfiResult<ClientCredentialsRs> {
        ClientCredentialsRs::try_from_base64_blob(&self.credentials_base64)
            .map_err(|e| anyhow::anyhow!("Invalid credentials: {e}").into())
    }
}

// ================= //
// --- Node Info --- //
// ================= //

/// Information about a Lexe node.
#[derive(Clone, uniffi::Record)]
pub struct NodeInfo {
    /// The node's current semver version, e.g. "0.6.9".
    pub version: String,
    /// Hex-encoded SGX measurement of the current node.
    pub measurement: String,
    /// Hex-encoded ed25519 user public key for this Lexe user.
    pub user_pk: String,
    /// Hex-encoded secp256k1 node public key ("node_id").
    pub node_pk: String,
    /// Total balance in sats (Lightning + on-chain).
    pub balance_sats: u64,
    /// Total Lightning balance in sats.
    pub lightning_balance_sats: u64,
    /// Estimated Lightning sendable balance in sats.
    pub lightning_sendable_balance_sats: u64,
    /// Maximum Lightning sendable balance in sats.
    pub lightning_max_sendable_balance_sats: u64,
    /// Total on-chain balance in sats (includes unconfirmed).
    pub onchain_balance_sats: u64,
    /// Trusted on-chain balance in sats.
    pub onchain_trusted_balance_sats: u64,
    /// Total number of Lightning channels.
    pub num_channels: u64,
    /// Number of usable Lightning channels.
    pub num_usable_channels: u64,
}

impl From<NodeInfoRs> for NodeInfo {
    fn from(info: NodeInfoRs) -> Self {
        Self {
            version: info.version.to_string(),
            measurement: info.measurement.to_string(),
            user_pk: info.user_pk.to_string(),
            node_pk: info.node_pk.to_string(),
            balance_sats: info.balance.sats_u64(),
            lightning_balance_sats: info.lightning_balance.sats_u64(),
            lightning_sendable_balance_sats: info
                .lightning_sendable_balance
                .sats_u64(),
            lightning_max_sendable_balance_sats: info
                .lightning_max_sendable_balance
                .sats_u64(),
            onchain_balance_sats: info.onchain_balance.sats_u64(),
            onchain_trusted_balance_sats: info
                .onchain_trusted_balance
                .sats_u64(),
            num_channels: info.num_channels as u64,
            num_usable_channels: info.num_usable_channels as u64,
        }
    }
}

// ======================= //
// --- AsyncLexeWallet --- //
// ======================= //

/// Top-level async handle to a Lexe wallet.
///
/// Exposes simple async APIs for managing a Lexe wallet.
/// For synchronous usage, use [`BlockingLexeWallet`].
#[derive(uniffi::Object)]
pub struct AsyncLexeWallet {
    inner: LexeWalletRs<lexe::wallet::WithDb>,
}

#[uniffi::export(async_runtime = "tokio")]
impl AsyncLexeWallet {
    /// Create a fresh wallet, deleting any existing local state for this user.
    /// Data for other users and environments is not affected.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn fresh(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let inner = LexeWalletRs::fresh(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self { inner }))
    }

    /// Load an existing wallet from local state.
    ///
    /// Raises [`LoadWalletError::NotFound`] if no local data exists for this
    /// user and environment. Use [`AsyncLexeWallet::fresh`] to create local
    /// state.
    ///
    /// If this returns [`LoadWalletError::NotFound`] and you are
    /// authenticating with [`RootSeed`]s, call [`signup`](Self::signup) after
    /// creating the wallet if you're not sure whether the user has been
    /// signed up with Lexe.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn load(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let maybe_inner = LexeWalletRs::load(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_inner {
            Some(inner) => Ok(Arc::new(Self { inner })),
            None => Err(LoadWalletError::NotFound),
        }
    }

    /// Load an existing wallet, or create a fresh one if no local data exists.
    /// If you are authenticating with client credentials, this is generally
    /// what you want to use.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn load_or_fresh(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let inner = LexeWalletRs::load_or_fresh(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self { inner }))
    }

    /// Registers this user with Lexe and provisions their node.
    ///
    /// Call this after creating the wallet for the first time. It is
    /// idempotent, so calling it again for an already-signed-up user is safe.
    ///
    /// **Important**: After signup, ensure the user's root seed is persisted!
    /// Without their seed, users lose access to their funds permanently.
    ///
    /// - `partner_pk`: Optional hex-encoded [`UserPk`] of your company account.
    ///   Set this to earn a share of fees from wallets you sign up.
    pub async fn signup(
        &self,
        root_seed: Arc<RootSeed>,
        partner_pk: Option<String>,
    ) -> FfiResult<()> {
        let partner = partner_pk
            .as_deref()
            .map(|s| {
                s.parse::<UserPk>().map_err(|e| {
                    anyhow::anyhow!("Invalid partner user_pk: {e}")
                })
            })
            .transpose()?;

        self.inner.signup(root_seed.as_rs(), partner).await?;
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the user is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub async fn provision(&self, root_seed: Arc<RootSeed>) -> FfiResult<()> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        self.inner.provision(credentials).await?;
        Ok(())
    }

    /// Get the user's hex-encoded public key derived from the root seed.
    pub fn user_pk(&self) -> String {
        self.inner.user_config().user_pk.to_string()
    }

    /// Get information about the node.
    pub async fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = self.inner.node_info().await?;
        Ok(info.into())
    }

    /// Create a BOLT11 invoice.
    /// `expiration_secs` is the invoice expiry, in seconds.
    /// `amount_sats` is optional; if `None`, the invoice is amountless.
    /// `description` is shown to the payer, if provided.
    /// `payer_note` is an optional note received from the payer out-of-band
    /// via LNURL-pay and is stored with this inbound payment. If provided, it
    /// must be non-empty and at most 200 chars / 512 UTF-8 bytes.
    #[uniffi::method(default(payer_note = None))]
    pub async fn create_invoice(
        &self,
        expiration_secs: u32,
        amount_sats: Option<u64>,
        description: Option<String>,
        payer_note: Option<String>,
    ) -> FfiResult<CreateInvoiceResponse> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid amount: {e}"))?;

        let req = CreateInvoiceRequestRs {
            expiration_secs,
            amount,
            description,
            payer_note,
        };
        let resp = self.inner.create_invoice(req).await?;
        Ok(resp.into())
    }

    /// Pay a BOLT11 invoice.
    /// `fallback_amount_sats` is required if the invoice is amountless.
    /// `note` is a private note that the receiver does not see.
    /// If provided, `note` must be non-empty and at most 200 chars / 512
    /// UTF-8 bytes.
    /// `payer_note` is an optional note that was sent to the receiver
    /// out-of-band via LNURL-pay and is visible to them. If provided,
    /// `payer_note` must be non-empty and at most 200 chars / 512 UTF-8
    /// bytes.
    #[uniffi::method(default(payer_note = None))]
    pub async fn pay_invoice(
        &self,
        invoice: String,
        fallback_amount_sats: Option<u64>,
        note: Option<String>,
        payer_note: Option<String>,
    ) -> FfiResult<PayInvoiceResponse> {
        let invoice: LxInvoiceRs = invoice
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid invoice: {e}"))?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid fallback amount: {e}"))?;

        let req = PayInvoiceRequestRs {
            invoice,
            fallback_amount,
            note,
            payer_note,
        };
        let resp = self.inner.pay_invoice(req).await?;
        Ok(resp.into())
    }

    /// Get a payment by its `index` string.
    pub async fn get_payment(
        &self,
        index: String,
    ) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = GetPaymentRequestRs { index };
        let resp = self.inner.get_payment(req).await?;
        Ok(resp.payment.map(Into::into))
    }

    /// Update a payment's note.
    /// Call `sync_payments` first so the payment exists locally.
    /// If `note` is `Some`, it must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    pub async fn update_payment_note(
        &self,
        index: String,
        note: Option<String>,
    ) -> FfiResult<()> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkUpdatePaymentNoteRequestRs { index, note };
        self.inner.update_payment_note(req).await?;
        Ok(())
    }

    /// Sync payments from the node to local storage.
    pub async fn sync_payments(&self) -> FfiResult<PaymentSyncSummary> {
        let summary = self.inner.sync_payments().await?;
        Ok(PaymentSyncSummary {
            num_new: summary.num_new as u64,
            num_updated: summary.num_updated as u64,
        })
    }

    /// List payments from local storage with cursor-based pagination.
    ///
    /// Defaults to descending order (newest first) with a limit of 100.
    ///
    /// To continue paginating, set `after` to the `next_index` from the
    /// previous response. `after` is an *exclusive* index.
    ///
    /// If needed, use `sync_payments` to fetch the latest data from the
    /// node before calling this method.
    #[uniffi::method(default(order = None, limit = None, after = None))]
    pub fn list_payments(
        &self,
        filter: PaymentFilter,
        order: Option<Order>,
        limit: Option<u32>,
        after: Option<String>,
    ) -> FfiResult<ListPaymentsResponse> {
        let filter_rs = filter.to_rs();
        let order_rs = order.map(|o| o.to_rs());
        let limit_rs = limit.map(|l| l as usize);
        let after_rs = after
            .map(|s| PaymentCreatedIndexRs::from_str(&s))
            .transpose()?;
        let resp = self.inner.list_payments(
            &filter_rs,
            order_rs,
            limit_rs,
            after_rs.as_ref(),
        );

        Ok(ListPaymentsResponse {
            payments: resp.payments.into_iter().map(Payment::from).collect(),
            next_index: resp.next_index.map(|idx| idx.to_string()),
        })
    }

    /// Clear all local payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call `sync_payments` to re-populate.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.inner.clear_payments()?;
        Ok(())
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 10 minutes if not specified.
    /// Maximum timeout is 86,400 seconds (24 hours).
    #[uniffi::method(default(timeout_secs = None))]
    pub async fn wait_for_payment(
        &self,
        index: String,
        timeout_secs: Option<u32>,
    ) -> FfiResult<Payment> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let timeout = timeout_secs.map(|s| Duration::from_secs(s.into()));
        let payment = self.inner.wait_for_payment(index, timeout).await?;
        Ok(Payment::from(payment))
    }
}

// ========================== //
// --- BlockingLexeWallet --- //
// ========================== //

/// Top-level synchronous handle to a Lexe wallet.
///
/// Exposes simple blocking APIs for managing a Lexe wallet.
/// For async usage, use [`AsyncLexeWallet`].
#[derive(uniffi::Object)]
pub struct BlockingLexeWallet {
    inner: BlockingLexeWalletRs,
}

#[uniffi::export]
impl BlockingLexeWallet {
    /// Create a fresh wallet, deleting any existing local state for this user.
    /// Data for other users and environments is not affected.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn fresh(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let inner = BlockingLexeWalletRs::fresh(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self { inner }))
    }

    /// Load an existing wallet from local state.
    ///
    /// Raises [`LoadWalletError::NotFound`] if no local data exists for this
    /// user and environment. Use [`BlockingLexeWallet::fresh`] to create local
    /// state.
    ///
    /// If this returns [`LoadWalletError::NotFound`] and you are
    /// authenticating with [`RootSeed`]s, call [`signup`](Self::signup) after
    /// creating the wallet if you're not sure whether the user has been
    /// signed up with Lexe.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn load(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let maybe_inner = BlockingLexeWalletRs::load(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_inner {
            Some(inner) => Ok(Arc::new(Self { inner })),
            None => Err(LoadWalletError::NotFound),
        }
    }

    /// Load an existing wallet, or create a fresh one if no local data exists.
    /// If you are authenticating with client credentials, this is generally
    /// what you want to use.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn load_or_fresh(
        env_config: Arc<WalletEnvConfig>,
        root_seed: Arc<RootSeed>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        let env_config_rs = env_config.to_rs();

        let inner = BlockingLexeWalletRs::load_or_fresh(
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self { inner }))
    }

    /// Registers this user with Lexe and provisions their node.
    ///
    /// Call this after creating the wallet for the first time. It is
    /// idempotent, so calling it again for an already-signed-up user is safe.
    ///
    /// **Important**: After signup, ensure the user's root seed is persisted!
    /// Without their seed, users lose access to their funds permanently.
    ///
    /// - `partner_pk`: Optional hex-encoded [`UserPk`] of your company account.
    ///   Set this to earn a share of fees from wallets you sign up.
    pub fn signup(
        &self,
        root_seed: Arc<RootSeed>,
        partner_pk: Option<String>,
    ) -> FfiResult<()> {
        let partner = partner_pk
            .as_deref()
            .map(|s| {
                s.parse::<UserPk>().map_err(|e| {
                    anyhow::anyhow!("Invalid partner user_pk: {e}")
                })
            })
            .transpose()?;

        self.inner.signup(root_seed.as_rs(), partner)?;
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the user is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub fn provision(&self, root_seed: Arc<RootSeed>) -> FfiResult<()> {
        let credentials = CredentialsRef::from(root_seed.as_rs());
        self.inner.provision(credentials)?;
        Ok(())
    }

    /// Get the user's hex-encoded public key derived from the root seed.
    pub fn user_pk(&self) -> String {
        self.inner.user_config().user_pk.to_string()
    }

    /// Get information about the node.
    pub fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = self.inner.node_info()?;
        Ok(info.into())
    }

    /// Create a BOLT11 invoice.
    /// `expiration_secs` is the invoice expiry, in seconds.
    /// `amount_sats` is optional; if `None`, the invoice is amountless.
    /// `description` is shown to the payer, if provided.
    /// `payer_note` is an optional note received from the payer out-of-band
    /// via LNURL-pay and is stored with this inbound payment. If provided, it
    /// must be non-empty and at most 200 chars / 512 UTF-8 bytes.
    #[uniffi::method(default(payer_note = None))]
    pub fn create_invoice(
        &self,
        expiration_secs: u32,
        amount_sats: Option<u64>,
        description: Option<String>,
        payer_note: Option<String>,
    ) -> FfiResult<CreateInvoiceResponse> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid amount: {e}"))?;

        let req = CreateInvoiceRequestRs {
            expiration_secs,
            amount,
            description,
            payer_note,
        };
        let resp = self.inner.create_invoice(req)?;
        Ok(resp.into())
    }

    /// Pay a BOLT11 invoice.
    /// `fallback_amount_sats` is required if the invoice is amountless.
    /// `note` is a private note that the receiver does not see.
    /// If provided, `note` must be non-empty and at most 200 chars / 512
    /// UTF-8 bytes.
    /// `payer_note` is an optional note that was sent to the receiver
    /// out-of-band via LNURL-pay and is visible to them. If provided,
    /// `payer_note` must be non-empty and at most 200 chars / 512 UTF-8
    /// bytes.
    #[uniffi::method(default(payer_note = None))]
    pub fn pay_invoice(
        &self,
        invoice: String,
        fallback_amount_sats: Option<u64>,
        note: Option<String>,
        payer_note: Option<String>,
    ) -> FfiResult<PayInvoiceResponse> {
        let invoice: LxInvoiceRs = invoice
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid invoice: {e}"))?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid fallback amount: {e}"))?;

        let req = PayInvoiceRequestRs {
            invoice,
            fallback_amount,
            note,
            payer_note,
        };
        let resp = self.inner.pay_invoice(req)?;
        Ok(resp.into())
    }

    /// Get a payment by its `index` string.
    pub fn get_payment(&self, index: String) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = GetPaymentRequestRs { index };
        let resp = self.inner.get_payment(req)?;
        Ok(resp.payment.map(Into::into))
    }

    /// Update a payment's note.
    /// Call `sync_payments` first so the payment exists locally.
    /// If `note` is `Some`, it must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    pub fn update_payment_note(
        &self,
        index: String,
        note: Option<String>,
    ) -> FfiResult<()> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkUpdatePaymentNoteRequestRs { index, note };
        self.inner.update_payment_note(req)?;
        Ok(())
    }

    /// Sync payments from the node to local storage.
    pub fn sync_payments(&self) -> FfiResult<PaymentSyncSummary> {
        let summary = self.inner.sync_payments()?;
        Ok(PaymentSyncSummary {
            num_new: summary.num_new as u64,
            num_updated: summary.num_updated as u64,
        })
    }

    /// List payments from local storage with cursor-based pagination.
    ///
    /// Defaults to descending order (newest first) with a limit of 100.
    ///
    /// To continue paginating, set `after` to the `next_index` from the
    /// previous response. `after` is an *exclusive* index.
    ///
    /// If needed, use `sync_payments` to fetch the latest data from the
    /// node before calling this method.
    #[uniffi::method(default(order = None, limit = None, after = None))]
    pub fn list_payments(
        &self,
        filter: PaymentFilter,
        order: Option<Order>,
        limit: Option<u32>,
        after: Option<String>,
    ) -> FfiResult<ListPaymentsResponse> {
        let filter_rs = filter.to_rs();
        let order_rs = order.map(|o| o.to_rs());
        let limit_rs = limit.map(|l| l as usize);
        let after_rs = after
            .map(|s| PaymentCreatedIndexRs::from_str(&s))
            .transpose()?;
        let resp = self.inner.list_payments(
            &filter_rs,
            order_rs,
            limit_rs,
            after_rs.as_ref(),
        );

        Ok(ListPaymentsResponse {
            payments: resp.payments.into_iter().map(Payment::from).collect(),
            next_index: resp.next_index.map(|idx| idx.to_string()),
        })
    }

    /// Clear all local payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call `sync_payments` to re-populate.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.inner.clear_payments()?;
        Ok(())
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 10 minutes if not specified.
    /// Maximum timeout is 86,400 seconds (24 hours).
    #[uniffi::method(default(timeout_secs = None))]
    pub fn wait_for_payment(
        &self,
        index: String,
        timeout_secs: Option<u32>,
    ) -> FfiResult<Payment> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let timeout = timeout_secs.map(|s| Duration::from_secs(s.into()));
        let payment = self.inner.wait_for_payment(index, timeout)?;
        Ok(Payment::from(payment))
    }
}

// ================ //
// --- Payments --- //
// ================ //

/// Confirmation priority for on-chain sends.
#[derive(Clone, uniffi::Enum)]
pub enum ConfirmationPriority {
    /// Fastest confirmation (highest fees).
    High,
    /// Standard confirmation target.
    Normal,
    /// Lowest fees (slowest confirmation).
    Background,
}

impl From<ConfirmationPriority> for ConfirmationPriorityRs {
    fn from(priority: ConfirmationPriority) -> Self {
        match priority {
            ConfirmationPriority::High => Self::High,
            ConfirmationPriority::Normal => Self::Normal,
            ConfirmationPriority::Background => Self::Background,
        }
    }
}

impl From<ConfirmationPriorityRs> for ConfirmationPriority {
    fn from(priority: ConfirmationPriorityRs) -> Self {
        match priority {
            ConfirmationPriorityRs::High => Self::High,
            ConfirmationPriorityRs::Normal => Self::Normal,
            ConfirmationPriorityRs::Background => Self::Background,
        }
    }
}

/// Direction of a payment.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentDirection {
    /// Incoming payment.
    Inbound,
    /// Outgoing payment.
    Outbound,
    /// Informational payment.
    Info,
}

impl From<PaymentDirectionRs> for PaymentDirection {
    fn from(direction: PaymentDirectionRs) -> Self {
        match direction {
            PaymentDirectionRs::Inbound => Self::Inbound,
            PaymentDirectionRs::Outbound => Self::Outbound,
            PaymentDirectionRs::Info => Self::Info,
        }
    }
}

/// Technical rail used to fulfill a payment.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentRail {
    /// On-chain Bitcoin payment.
    Onchain,
    /// Lightning invoice payment.
    Invoice,
    /// Lightning offer payment.
    Offer,
    /// Spontaneous Lightning payment.
    Spontaneous,
    /// Waived fee payment.
    WaivedFee,
    /// Unknown rail from a newer version of node.
    Unknown,
}

impl From<PaymentRailRs> for PaymentRail {
    fn from(rail: PaymentRailRs) -> Self {
        match rail {
            PaymentRailRs::Onchain => Self::Onchain,
            PaymentRailRs::Invoice => Self::Invoice,
            PaymentRailRs::Offer => Self::Offer,
            PaymentRailRs::Spontaneous => Self::Spontaneous,
            PaymentRailRs::WaivedFee => Self::WaivedFee,
            PaymentRailRs::Unknown(_) => Self::Unknown,
        }
    }
}

/// Status of a payment.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentStatus {
    /// Payment is pending.
    Pending,
    /// Payment completed successfully.
    Completed,
    /// Payment failed.
    Failed,
}

impl From<PaymentStatusRs> for PaymentStatus {
    fn from(status: PaymentStatusRs) -> Self {
        match status {
            PaymentStatusRs::Pending => Self::Pending,
            PaymentStatusRs::Completed => Self::Completed,
            PaymentStatusRs::Failed => Self::Failed,
        }
    }
}

/// Filter for listing payments.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentFilter {
    /// Include all payments.
    All,
    /// Include only pending payments.
    Pending,
    /// Include only completed payments.
    Completed,
    /// Include only failed payments.
    Failed,
    /// Include only finalized payments (completed or failed).
    Finalized,
}

impl PaymentFilter {
    fn to_rs(&self) -> lexe::types::payment::PaymentFilter {
        match self {
            Self::All => lexe::types::payment::PaymentFilter::All,
            Self::Pending => lexe::types::payment::PaymentFilter::Pending,
            Self::Completed => lexe::types::payment::PaymentFilter::Completed,
            Self::Failed => lexe::types::payment::PaymentFilter::Failed,
            Self::Finalized => lexe::types::payment::PaymentFilter::Finalized,
        }
    }
}

/// Sort order for listing results.
#[derive(Clone, uniffi::Enum)]
pub enum Order {
    /// Ascending order (oldest first).
    Asc,
    /// Descending order (newest first).
    Desc,
}

impl Order {
    fn to_rs(&self) -> lexe::types::payment::Order {
        match self {
            Self::Asc => lexe::types::payment::Order::Asc,
            Self::Desc => lexe::types::payment::Order::Desc,
        }
    }
}

/// Application-level kind for a payment.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentKind {
    /// On-chain payment.
    Onchain,
    /// Lightning invoice payment.
    Invoice,
    /// Lightning offer payment.
    Offer,
    /// Spontaneous Lightning payment.
    Spontaneous,
    /// Waived channel fee payment.
    WaivedChannelFee,
    /// Waived liquidity fee payment.
    WaivedLiquidityFee,
    /// Unknown kind from a newer version of node.
    Unknown { inner: String },
}

impl From<PaymentKindRs> for PaymentKind {
    fn from(kind: PaymentKindRs) -> Self {
        match kind {
            PaymentKindRs::Onchain => Self::Onchain,
            PaymentKindRs::Invoice => Self::Invoice,
            PaymentKindRs::Offer => Self::Offer,
            PaymentKindRs::Spontaneous => Self::Spontaneous,
            PaymentKindRs::WaivedChannelFee => Self::WaivedChannelFee,
            PaymentKindRs::WaivedLiquidityFee => Self::WaivedLiquidityFee,
            PaymentKindRs::Unknown(s) => Self::Unknown {
                inner: String::from(s),
            },
        }
    }
}

/// A BOLT11 Lightning invoice.
#[derive(Clone, uniffi::Record)]
pub struct Invoice {
    /// The full invoice string (bech32 encoded).
    pub string: String,
    /// Invoice description, if present.
    pub description: Option<String>,
    /// Creation timestamp (milliseconds since the UNIX epoch).
    pub created_at_ms: u64,
    /// Expiration timestamp (milliseconds since the UNIX epoch).
    pub expires_at_ms: u64,
    /// Amount in satoshis, if specified.
    pub amount_sats: Option<u64>,
    /// The payee's node public key (hex-encoded).
    pub payee_pubkey: String,
}

impl From<&LxInvoiceRs> for Invoice {
    fn from(invoice: &LxInvoiceRs) -> Self {
        Self {
            string: invoice.to_string(),
            description: invoice.description_str().map(String::from),
            created_at_ms: invoice.saturating_created_at().to_millis(),
            expires_at_ms: invoice.saturating_expires_at().to_millis(),
            amount_sats: invoice.amount_sats(),
            payee_pubkey: invoice.payee_node_pk().to_string(),
        }
    }
}

impl From<&Arc<LxInvoiceRs>> for Invoice {
    fn from(invoice: &Arc<LxInvoiceRs>) -> Self {
        Self::from(invoice.as_ref())
    }
}

/// Information about a payment.
#[derive(Clone, uniffi::Record)]
pub struct Payment {
    /// Unique payment identifier, ordered by `created_at_ms`.
    /// Format: `<created_at_ms>-<payment_id>`.
    pub index: String,
    /// Timestamp when payment was created (milliseconds since the UNIX
    /// epoch).
    pub created_at_ms: u64,
    /// Timestamp when payment was last updated (milliseconds since the UNIX
    /// epoch).
    pub updated_at_ms: u64,
    /// Technical rail used to fulfill this payment.
    pub rail: PaymentRail,
    /// Application-level payment kind.
    pub kind: PaymentKind,
    /// Payment direction: inbound, outbound, or info.
    pub direction: PaymentDirection,
    /// Payment status.
    pub status: PaymentStatus,
    /// Human-readable payment status message.
    pub status_msg: String,
    /// Payment amount in satoshis, if known.
    pub amount_sats: Option<u64>,
    /// Fees paid in satoshis.
    pub fees_sats: u64,
    /// Optional personal note attached to this payment.
    pub note: Option<String>,
    /// BOLT11 invoice used for this payment, if any.
    pub invoice: Option<Invoice>,
    /// Hex-encoded Bitcoin txid (on-chain payments only).
    pub txid: Option<String>,
    /// Bitcoin address for on-chain sends.
    pub address: Option<String>,
    /// Invoice or offer expiry time (milliseconds since the UNIX epoch).
    pub expires_at_ms: Option<u64>,
    /// When this payment finalized (milliseconds since the UNIX epoch).
    pub finalized_at_ms: Option<u64>,
    /// (Offer payments) Payer's self-reported name.
    pub payer_name: Option<String>,
    /// (Offer payments) Payer's provided note.
    pub payer_note: Option<String>,
    /// (On-chain sends) Confirmation priority for this payment.
    pub priority: Option<ConfirmationPriority>,
}

impl From<PaymentRs> for Payment {
    fn from(payment: PaymentRs) -> Self {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the conversion below.
        let PaymentRs {
            index,
            rail,
            kind,
            direction,
            txid,
            amount,
            fees,
            status,
            status_msg,
            address,
            invoice,
            tx: _,
            note,
            payer_name,
            payer_note,
            priority,
            expires_at,
            finalized_at,
            created_at,
            updated_at,
        } = payment;

        Self {
            index: index.to_string(),
            created_at_ms: created_at.to_millis(),
            updated_at_ms: updated_at.to_millis(),
            rail: rail.into(),
            kind: kind.into(),
            direction: direction.into(),
            status: status.into(),
            status_msg,
            amount_sats: amount.map(|a| a.sats_u64()),
            fees_sats: fees.sats_u64(),
            note,
            invoice: invoice.as_ref().map(Invoice::from),
            txid: txid.map(|t| t.to_string()),
            address: address
                .as_ref()
                .map(|a| a.assume_checked_ref().to_string()),
            expires_at_ms: expires_at.map(|t| t.to_millis()),
            finalized_at_ms: finalized_at.map(|t| t.to_millis()),
            payer_name,
            payer_note,
            priority: priority.map(Into::into),
        }
    }
}

/// Summary of a payment sync operation.
#[derive(Clone, uniffi::Record)]
pub struct PaymentSyncSummary {
    /// Number of new payments added to the local DB.
    pub num_new: u64,
    /// Number of existing payments that were updated.
    pub num_updated: u64,
}

/// Response from listing payments.
#[derive(Clone, uniffi::Record)]
pub struct ListPaymentsResponse {
    /// Payments in the requested page.
    pub payments: Vec<Payment>,
    /// Cursor for fetching the next page. `None` when there are no more
    /// results. Pass this as the `after` argument to get the next page.
    pub next_index: Option<String>,
}

// ================ //
// --- Invoices --- //
// ================ //

/// Response from creating an invoice.
#[derive(Clone, uniffi::Record)]
pub struct CreateInvoiceResponse {
    /// Unique payment identifier for this invoice.
    pub index: String,
    /// String-encoded BOLT11 invoice.
    pub invoice: String,
    /// Description encoded in the invoice, if provided.
    pub description: Option<String>,
    /// Amount encoded in the invoice, in satoshis (if any).
    pub amount_sats: Option<u64>,
    /// Invoice creation time (milliseconds since the UNIX epoch).
    pub created_at_ms: u64,
    /// Invoice expiration time (milliseconds since the UNIX epoch).
    pub expires_at_ms: u64,
    /// Hex-encoded payment hash.
    pub payment_hash: String,
    /// Payment secret for the invoice.
    pub payment_secret: String,
}

impl From<CreateInvoiceResponseRs> for CreateInvoiceResponse {
    fn from(resp: CreateInvoiceResponseRs) -> Self {
        Self {
            index: resp.index.to_string(),
            invoice: resp.invoice.to_string(),
            description: resp.description,
            amount_sats: resp.amount.map(|a| a.sats_u64()),
            created_at_ms: resp.created_at.to_millis(),
            expires_at_ms: resp.expires_at.to_millis(),
            payment_hash: resp.payment_hash.to_string(),
            payment_secret: resp.payment_secret.to_string(),
        }
    }
}

/// Response from paying an invoice.
#[derive(Clone, uniffi::Record)]
pub struct PayInvoiceResponse {
    /// Unique payment identifier for this payment.
    pub index: String,
    /// When we tried to pay this invoice (milliseconds since the UNIX
    /// epoch).
    pub created_at_ms: u64,
}

impl From<PayInvoiceResponseRs> for PayInvoiceResponse {
    fn from(resp: PayInvoiceResponseRs) -> Self {
        Self {
            index: resp.index.to_string(),
            created_at_ms: resp.created_at.to_millis(),
        }
    }
}
