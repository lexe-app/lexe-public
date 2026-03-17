//! Lexe SDK foreign language bindings.
//!
//! This crate is the [UniFFI] base for generating Lexe SDK bindings in
//! languages like Python, Javascript, Swift, and Kotlin.
//!
//! For Rust projects, use the [`lexe`] crate directly.
//!
//! [UniFFI]: https://mozilla.github.io/uniffi-rs/

use std::{fmt, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use anyhow::anyhow;
use lexe::{
    blocking_wallet::BlockingLexeWallet as SdkBlockingLexeWallet,
    config::WalletEnvConfig as SdkWalletEnvConfig,
    types::{
        auth::{
            ClientCredentials as SdkClientCredentials,
            CredentialsRef as SdkCredentialsRef, RootSeed as SdkRootSeed,
            UserPk,
        },
        command::{
            CreateInvoiceRequest as SdkCreateInvoiceRequest,
            CreateInvoiceResponse as SdkCreateInvoiceResponse,
            GetPaymentRequest as SdkGetPaymentRequest, NodeInfo as SdkNodeInfo,
            PayInvoiceRequest as SdkPayInvoiceRequest,
            PayInvoiceResponse as SdkPayInvoiceResponse,
            UpdatePaymentNoteRequest,
        },
        payment::Payment as SdkPayment,
    },
    wallet::{LexeWallet as SdkLexeWallet, WithDb, WithoutDb},
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
use lexe_common::{
    env::DeployEnv as DeployEnvRs,
    ln::{
        amount::Amount as AmountRs, network::LxNetwork as LxNetworkRs,
        priority::ConfirmationPriority as ConfirmationPriorityRs,
    },
};
use secrecy::Zeroize;

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
/// Returned by [`RootSeed`] file I/O methods.
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
}

impl WalletEnvConfig {
    // TODO(max): Could all of these to_rs be `From` impls?
    fn to_rs(&self) -> SdkWalletEnvConfig {
        match self.deploy_env {
            DeployEnv::Prod => SdkWalletEnvConfig::mainnet(),
            DeployEnv::Staging => SdkWalletEnvConfig::testnet3(),
            DeployEnv::Dev => SdkWalletEnvConfig::regtest(
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
    sdk: SdkRootSeed,
}

#[uniffi::export]
impl RootSeed {
    // --- Constructors & File I/O --- //

    /// Generate a new random root seed.
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self {
            sdk: SdkRootSeed::generate(),
        })
    }

    /// Reads a root seed from `~/.lexe/seedphrase[.env].txt`.
    ///
    /// Raises [`SeedFileError::NotFound`] if the file doesn't exist,
    /// or [`SeedFileError::ParseError`] if the file can't be parsed.
    #[uniffi::constructor]
    pub fn read(
        env_config: Arc<WalletEnvConfig>,
    ) -> Result<Arc<Self>, SeedFileError> {
        let wallet_env = env_config.to_rs().wallet_env;
        let sdk = match SdkRootSeed::read(&wallet_env) {
            Ok(Some(sdk)) => sdk,
            Ok(None) => {
                let data_dir = default_lexe_data_dir().unwrap_or_default();
                let path = env_config.seedphrase_path(data_dir);
                return Err(SeedFileError::NotFound { path });
            }
            Err(e) =>
                return Err(SeedFileError::ParseError {
                    message: format!("{e:#}"),
                }),
        };
        Ok(Arc::new(Self { sdk }))
    }

    /// Writes this root seed's mnemonic to `~/.lexe/seedphrase[.env].txt`.
    ///
    /// Creates parent directories if needed. Raises
    /// [`SeedFileError::AlreadyExists`] if the file already exists.
    pub fn write(
        &self,
        env_config: Arc<WalletEnvConfig>,
    ) -> Result<(), SeedFileError> {
        let wallet_env = env_config.to_rs().wallet_env;
        self.as_sdk().write(&wallet_env).map_err(|e| {
            // Check if the root cause is an "already exists" IO error.
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::AlreadyExists
                {
                    let data_dir = default_lexe_data_dir().unwrap_or_default();
                    let path = env_config.seedphrase_path(data_dir);
                    return SeedFileError::AlreadyExists { path };
                }
            }
            SeedFileError::IoError {
                message: format!("{e:#}"),
            }
        })
    }

    /// Reads a root seed from a seedphrase file at a specific path,
    /// containing a BIP39 mnemonic.
    ///
    /// Raises [`SeedFileError::NotFound`] if the file doesn't exist,
    /// or [`SeedFileError::ParseError`] if the file can't be parsed.
    #[uniffi::constructor]
    pub fn read_from_path(path: String) -> Result<Arc<Self>, SeedFileError> {
        let sdk = match SdkRootSeed::read_from_path(path.as_ref()) {
            Ok(Some(sdk)) => sdk,
            Ok(None) => return Err(SeedFileError::NotFound { path }),
            Err(e) =>
                return Err(SeedFileError::ParseError {
                    message: format!("{e:#}"),
                }),
        };

        Ok(Arc::new(Self { sdk }))
    }

    /// Writes this root seed's mnemonic to a seedphrase file at a specific
    /// path.
    ///
    /// Creates parent directories if needed. Raises
    /// [`SeedFileError::AlreadyExists`] if the file already exists.
    pub fn write_to_path(&self, path: String) -> Result<(), SeedFileError> {
        self.as_sdk().write_to_path(path.as_ref()).map_err(|e| {
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

    /// Construct a root seed from a BIP39 mnemonic string.
    #[uniffi::constructor]
    pub fn from_mnemonic(mnemonic: String) -> FfiResult<Arc<Self>> {
        let mnemonic = mnemonic
            .parse()
            .map_err(|e| anyhow!("Invalid mnemonic: {e}"))?;
        let sdk = SdkRootSeed::from_mnemonic(mnemonic)?;
        Ok(Arc::new(Self { sdk }))
    }

    /// Construct a root seed from raw bytes.
    ///
    /// The seed must be exactly 32 bytes.
    #[uniffi::constructor]
    pub fn from_bytes(mut seed_bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        let sdk = SdkRootSeed::try_from(seed_bytes.as_slice())?;
        seed_bytes.zeroize();
        Ok(Arc::new(Self { sdk }))
    }

    /// Construct a root seed from a 64-character hex string.
    #[uniffi::constructor]
    pub fn from_hex(hex_string: String) -> FfiResult<Arc<Self>> {
        let sdk = SdkRootSeed::from_hex(&hex_string)?;
        Ok(Arc::new(Self { sdk }))
    }

    // --- Serialization --- //

    /// Return the 32-byte root seed.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.sdk.as_bytes().to_vec()
    }

    /// Encode the root secret as a 64-character hex string.
    pub fn to_hex(&self) -> String {
        self.sdk.to_hex()
    }

    /// Return this root seed's mnemonic as a space-separated string.
    pub fn to_mnemonic(&self) -> String {
        self.sdk.to_mnemonic().to_string()
    }

    // --- Derived Identity --- //

    /// Derive the user's public key.
    pub fn derive_user_pk(&self) -> String {
        self.sdk.derive_user_pk().to_string()
    }

    /// Derive the node public key.
    pub fn derive_node_pk(&self) -> String {
        self.sdk.derive_node_pk().to_string()
    }

    // --- Encryption --- //

    /// Encrypt this root seed under the given password.
    pub fn password_encrypt(&self, password: String) -> FfiResult<Vec<u8>> {
        self.sdk.password_encrypt(&password).map_err(Into::into)
    }

    /// Decrypt a password-encrypted root seed.
    #[uniffi::constructor]
    pub fn password_decrypt(
        password: String,
        encrypted: Vec<u8>,
    ) -> FfiResult<Arc<Self>> {
        let sdk = SdkRootSeed::password_decrypt(&password, encrypted)?;
        Ok(Arc::new(Self { sdk }))
    }
}

impl RootSeed {
    fn as_sdk(&self) -> &SdkRootSeed {
        &self.sdk
    }
}

/// Scoped and revocable credentials for controlling a Lexe user node.
///
/// These are useful when you want node access without exposing the user's
/// [`RootSeed`], which is irrevocable.
/// Wrap into [`Credentials`] to use with wallet constructors.
#[derive(uniffi::Object)]
#[uniffi::export(Display)]
pub struct ClientCredentials {
    sdk: SdkClientCredentials,
}

#[uniffi::export]
impl ClientCredentials {
    /// Parse client credentials from a string.
    #[uniffi::constructor]
    pub fn from_string(s: String) -> FfiResult<Arc<Self>> {
        let sdk = SdkClientCredentials::from_string(&s)?;
        Ok(Arc::new(Self { sdk }))
    }

    /// Export these credentials as a portable string.
    ///
    /// The returned string can be passed to [`ClientCredentials::from_string`]
    /// to reconstruct the credentials.
    //
    // Intentional departure: the `lexe` SDK has both `Display` and
    // `export_string` (which wraps `to_string()`). UniFFI exports
    // `Display` too, but `export_string` is more discoverable for
    // SDK consumers who may not know about `Display` / `__str__`.
    pub fn export_string(&self) -> String {
        self.sdk.export_string()
    }
}

impl fmt::Display for ClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.sdk)
    }
}

impl ClientCredentials {
    fn as_sdk(&self) -> &SdkClientCredentials {
        &self.sdk
    }
}

/// Credentials for authenticating with a Lexe user node.
///
/// Create from a [`RootSeed`] or [`ClientCredentials`], then pass to
/// wallet constructors and [`provision`](AsyncLexeWallet::provision).
//
// Language-agnostic adaptation of the Rust SDK's `CredentialsRef<'_>`.
// Root-seed-only APIs like `signup` intentionally still take `RootSeed`.
#[derive(uniffi::Object)]
pub struct Credentials {
    inner: CredentialsInner,
}

enum CredentialsInner {
    RootSeed(Arc<RootSeed>),
    ClientCredentials(Arc<ClientCredentials>),
}

#[uniffi::export]
impl Credentials {
    /// Create credentials from a root seed.
    #[uniffi::constructor]
    pub fn from_root_seed(root_seed: Arc<RootSeed>) -> Arc<Self> {
        Arc::new(Self {
            inner: CredentialsInner::RootSeed(root_seed),
        })
    }

    /// Create credentials from client credentials.
    #[uniffi::constructor]
    pub fn from_client_credentials(
        client_credentials: Arc<ClientCredentials>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: CredentialsInner::ClientCredentials(client_credentials),
        })
    }
}

impl Credentials {
    fn as_sdk(&self) -> SdkCredentialsRef<'_> {
        match &self.inner {
            CredentialsInner::RootSeed(rs) =>
                SdkCredentialsRef::from(rs.as_sdk()),
            CredentialsInner::ClientCredentials(cc) =>
                SdkCredentialsRef::from(cc.as_sdk()),
        }
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

impl From<SdkNodeInfo> for NodeInfo {
    fn from(info: SdkNodeInfo) -> Self {
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
    inner: AsyncLexeWalletInner,
}

enum AsyncLexeWalletInner {
    WithDb(SdkLexeWallet<WithDb>),
    WithoutDb(SdkLexeWallet<WithoutDb>),
}

impl AsyncLexeWallet {
    /// Returns the inner `WithDb` wallet, or an error if this wallet was
    /// created without local persistence.
    fn with_db(&self) -> FfiResult<&SdkLexeWallet<WithDb>> {
        match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) => Ok(wallet),
            AsyncLexeWalletInner::WithoutDb(_) => Err(anyhow!(
                "This wallet was created without local persistence"
            )
            .into()),
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AsyncLexeWallet {
    // --- Constructors --- //

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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet = SdkLexeWallet::fresh(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: AsyncLexeWalletInner::WithDb(sdk_wallet),
        }))
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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let env_config_rs = env_config.to_rs();

        let maybe_sdk_wallet = SdkLexeWallet::load(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_sdk_wallet {
            Some(sdk_wallet) => Ok(Arc::new(Self {
                inner: AsyncLexeWalletInner::WithDb(sdk_wallet),
            })),
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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet = SdkLexeWallet::load_or_fresh(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: AsyncLexeWalletInner::WithDb(sdk_wallet),
        }))
    }

    /// Create a wallet without local persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations (`sync_payments`, `list_payments`,
    /// `clear_payments`) are not available and will return an error if called.
    #[uniffi::constructor]
    pub fn without_db(
        env_config: Arc<WalletEnvConfig>,
        credentials: Arc<Credentials>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet =
            SdkLexeWallet::without_db(env_config_rs, credentials.as_sdk())?;

        Ok(Arc::new(Self {
            inner: AsyncLexeWalletInner::WithoutDb(sdk_wallet),
        }))
    }

    // --- Shared methods (WithDb + WithoutDb) --- //

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
                s.parse::<UserPk>()
                    .map_err(|e| anyhow!("Invalid partner user_pk: {e}"))
            })
            .transpose()?;

        match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.signup(root_seed.as_sdk(), partner).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.signup(root_seed.as_sdk(), partner).await?,
        }
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the user is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub async fn provision(
        &self,
        credentials: Arc<Credentials>,
    ) -> FfiResult<()> {
        match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.provision(credentials.as_sdk()).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.provision(credentials.as_sdk()).await?,
        }
        Ok(())
    }

    /// Get the user's hex-encoded public key.
    pub fn user_pk(&self) -> String {
        match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.user_config().user_pk.to_string(),
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.user_config().user_pk.to_string(),
        }
    }

    /// Get information about the node.
    pub async fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) => wallet.node_info().await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.node_info().await?,
        };
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
            .map_err(|e| anyhow!("Invalid amount: {e}"))?;

        let req = SdkCreateInvoiceRequest {
            expiration_secs,
            amount,
            description,
            payer_note,
        };
        let resp = match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.create_invoice(req).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.create_invoice(req).await?,
        };
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
            .map_err(|e| anyhow!("Invalid invoice: {e}"))?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow!("Invalid fallback amount: {e}"))?;

        let req = SdkPayInvoiceRequest {
            invoice,
            fallback_amount,
            note,
            payer_note,
        };
        let resp = match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.pay_invoice(req).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.pay_invoice(req).await?,
        };
        Ok(resp.into())
    }

    /// Get a payment by its `index` string.
    pub async fn get_payment(
        &self,
        index: String,
    ) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkGetPaymentRequest { index };
        let resp = match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.get_payment(req).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.get_payment(req).await?,
        };
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
        let req = UpdatePaymentNoteRequest { index, note };
        match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.update_payment_note(req).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.update_payment_note(req).await?,
        }
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
        let timeout = timeout_secs.map(|secs| Duration::from_secs(secs.into()));
        let payment = match &self.inner {
            AsyncLexeWalletInner::WithDb(wallet) =>
                wallet.wait_for_payment(index, timeout).await?,
            AsyncLexeWalletInner::WithoutDb(wallet) =>
                wallet.wait_for_payment(index, timeout).await?,
        };
        Ok(Payment::from(payment))
    }

    // --- DB-only methods --- //

    /// Sync payments from the node to local storage.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    pub async fn sync_payments(&self) -> FfiResult<PaymentSyncSummary> {
        let summary = self.with_db()?.sync_payments().await?;
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    #[uniffi::method(default(order = None, limit = None, after = None))]
    pub fn list_payments(
        &self,
        filter: PaymentFilter,
        order: Option<Order>,
        limit: Option<u32>,
        after: Option<String>,
    ) -> FfiResult<ListPaymentsResponse> {
        let sdk_wallet = self.with_db()?;
        let filter_rs = filter.to_rs();
        let order_rs = order.map(|o| o.to_rs());
        let limit_rs = limit.map(|l| l as usize);
        let after_rs = after
            .map(|s| PaymentCreatedIndexRs::from_str(&s))
            .transpose()?;
        let resp = sdk_wallet.list_payments(
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.with_db()?.clear_payments()?;
        Ok(())
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
    inner: BlockingLexeWalletInner,
}

enum BlockingLexeWalletInner {
    WithDb(SdkBlockingLexeWallet<WithDb>),
    WithoutDb(SdkBlockingLexeWallet<WithoutDb>),
}

impl BlockingLexeWallet {
    /// Returns the inner `WithDb` wallet, or an error if this wallet was
    /// created without local persistence.
    fn with_db(&self) -> FfiResult<&SdkBlockingLexeWallet<WithDb>> {
        match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) => Ok(wallet),
            BlockingLexeWalletInner::WithoutDb(_) => Err(anyhow!(
                "This wallet was created without local persistence"
            )
            .into()),
        }
    }
}

#[uniffi::export]
impl BlockingLexeWallet {
    // --- Constructors --- //

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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet = SdkBlockingLexeWallet::fresh(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: BlockingLexeWalletInner::WithDb(sdk_wallet),
        }))
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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let env_config_rs = env_config.to_rs();

        let maybe_sdk_wallet = SdkBlockingLexeWallet::load(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_sdk_wallet {
            Some(sdk_wallet) => Ok(Arc::new(Self {
                inner: BlockingLexeWalletInner::WithDb(sdk_wallet),
            })),
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
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet = SdkBlockingLexeWallet::load_or_fresh(
            env_config_rs,
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: BlockingLexeWalletInner::WithDb(sdk_wallet),
        }))
    }

    /// Create a wallet without local persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations (`sync_payments`, `list_payments`,
    /// `clear_payments`) are not available and will return an error if called.
    #[uniffi::constructor]
    pub fn without_db(
        env_config: Arc<WalletEnvConfig>,
        credentials: Arc<Credentials>,
    ) -> FfiResult<Arc<Self>> {
        let env_config_rs = env_config.to_rs();

        let sdk_wallet = SdkBlockingLexeWallet::without_db(
            env_config_rs,
            credentials.as_sdk(),
        )?;

        Ok(Arc::new(Self {
            inner: BlockingLexeWalletInner::WithoutDb(sdk_wallet),
        }))
    }

    // --- Shared methods (WithDb + WithoutDb) --- //

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
                s.parse::<UserPk>()
                    .map_err(|e| anyhow!("Invalid partner user_pk: {e}"))
            })
            .transpose()?;

        match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.signup(root_seed.as_sdk(), partner)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.signup(root_seed.as_sdk(), partner)?,
        }
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the user is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub fn provision(&self, credentials: Arc<Credentials>) -> FfiResult<()> {
        match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.provision(credentials.as_sdk())?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.provision(credentials.as_sdk())?,
        }
        Ok(())
    }

    /// Get the user's hex-encoded public key.
    pub fn user_pk(&self) -> String {
        match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.user_config().user_pk.to_string(),
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.user_config().user_pk.to_string(),
        }
    }

    /// Get information about the node.
    pub fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) => wallet.node_info()?,
            BlockingLexeWalletInner::WithoutDb(wallet) => wallet.node_info()?,
        };
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
            .map_err(|e| anyhow!("Invalid amount: {e}"))?;

        let req = SdkCreateInvoiceRequest {
            expiration_secs,
            amount,
            description,
            payer_note,
        };
        let resp = match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.create_invoice(req)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.create_invoice(req)?,
        };
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
            .map_err(|e| anyhow!("Invalid invoice: {e}"))?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow!("Invalid fallback amount: {e}"))?;

        let req = SdkPayInvoiceRequest {
            invoice,
            fallback_amount,
            note,
            payer_note,
        };
        let resp = match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.pay_invoice(req)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.pay_invoice(req)?,
        };
        Ok(resp.into())
    }

    /// Get a payment by its `index` string.
    pub fn get_payment(&self, index: String) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkGetPaymentRequest { index };
        let resp = match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.get_payment(req)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.get_payment(req)?,
        };
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
        let req = UpdatePaymentNoteRequest { index, note };
        match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.update_payment_note(req)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.update_payment_note(req)?,
        }
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
        let timeout = timeout_secs.map(|secs| Duration::from_secs(secs.into()));
        let payment = match &self.inner {
            BlockingLexeWalletInner::WithDb(wallet) =>
                wallet.wait_for_payment(index, timeout)?,
            BlockingLexeWalletInner::WithoutDb(wallet) =>
                wallet.wait_for_payment(index, timeout)?,
        };
        Ok(Payment::from(payment))
    }

    // --- DB-only methods --- //

    /// Sync payments from the node to local storage.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    pub fn sync_payments(&self) -> FfiResult<PaymentSyncSummary> {
        let summary = self.with_db()?.sync_payments()?;
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    #[uniffi::method(default(order = None, limit = None, after = None))]
    pub fn list_payments(
        &self,
        filter: PaymentFilter,
        order: Option<Order>,
        limit: Option<u32>,
        after: Option<String>,
    ) -> FfiResult<ListPaymentsResponse> {
        let sdk_wallet = self.with_db()?;
        let filter_rs = filter.to_rs();
        let order_rs = order.map(|o| o.to_rs());
        let limit_rs = limit.map(|l| l as usize);
        let after_rs = after
            .map(|s| PaymentCreatedIndexRs::from_str(&s))
            .transpose()?;
        let resp = sdk_wallet.list_payments(
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error for wallets created with `without_db`.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.with_db()?.clear_payments()?;
        Ok(())
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

impl From<SdkPayment> for Payment {
    fn from(payment: SdkPayment) -> Self {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the conversion below.
        let SdkPayment {
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

impl From<SdkCreateInvoiceResponse> for CreateInvoiceResponse {
    fn from(resp: SdkCreateInvoiceResponse) -> Self {
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

impl From<SdkPayInvoiceResponse> for PayInvoiceResponse {
    fn from(resp: SdkPayInvoiceResponse) -> Self {
        Self {
            index: resp.index.to_string(),
            created_at_ms: resp.created_at.to_millis(),
        }
    }
}
