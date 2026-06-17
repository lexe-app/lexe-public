//! Lexe SDK foreign language bindings.
//!
//! This crate is the [UniFFI] base for generating Lexe SDK bindings in
//! languages like Python, Javascript, Swift, and Kotlin.
//!
//! For Rust projects, use the [`lexe`] crate directly.
//!
//! [UniFFI]: https://mozilla.github.io/uniffi-rs/

use std::{fmt, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use lexe::{
    bip39::Mnemonic,
    blocking_wallet::BlockingLexeWallet as SdkBlockingLexeWallet,
    config::WalletEnvConfig as SdkWalletEnvConfig,
    types::{
        auth::{
            ClientCredentials as SdkClientCredentials,
            CredentialsRef as SdkCredentialsRef, RootSeed as SdkRootSeed,
            UserPk,
        },
        bitcoin::{
            LnurlPayRequest as SdkLnurlPayRequest,
            LnurlPayRequestMetadata as SdkLnurlPayRequestMetadata,
            PaymentMethod as SdkPaymentMethod,
        },
        command::{
            AnalyzeRequest as SdkAnalyzeRequest,
            AnalyzeResponse as SdkAnalyzeResponse,
            CreateInvoiceRequest as SdkCreateInvoiceRequest,
            CreateInvoiceResponse as SdkCreateInvoiceResponse,
            CreateOfferRequest as SdkCreateOfferRequest,
            CreateOfferResponse as SdkCreateOfferResponse,
            GetPaymentRequest as SdkGetPaymentRequest,
            GetUpdatedPaymentsRequest as SdkGetUpdatedPaymentsRequest,
            GetUpdatedPaymentsResponse as SdkGetUpdatedPaymentsResponse,
            NodeInfo as SdkNodeInfo, PayInvoiceRequest as SdkPayInvoiceRequest,
            PayLnurlRequest as SdkPayLnurlRequest,
            PayOfferRequest as SdkPayOfferRequest, PayRequest as SdkPayRequest,
            PayableDetails as SdkPayableDetails, UpdatePersonalNoteRequest,
            WithdrawLnurlRequest as SdkWithdrawLnurlRequest,
        },
        payment::Payment as SdkPayment,
        util::Ppm,
    },
    wallet::LexeWallet as SdkLexeWallet,
};
use lexe_api_core::{
    error::GatewayApiError as GatewayApiErrorRs,
    types::{
        invoice::Invoice as InvoiceRs,
        offer::Offer as OfferRs,
        payments::{
            ClientPaymentId as ClientPaymentIdRs,
            PaymentCreatedIndex as PaymentCreatedIndexRs,
            PaymentDirection as PaymentDirectionRs,
            PaymentKind as PaymentKindRs, PaymentRail as PaymentRailRs,
            PaymentStatus as PaymentStatusRs,
            PaymentUpdatedIndex as PaymentUpdatedIndexRs,
        },
    },
};
use lexe_common::{
    ByteArray,
    env::DeployEnv as DeployEnvRs,
    ln::{
        amount::Amount as AmountRs, network::Network as NetworkRs,
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
#[uniffi::export(default(default_level = "info"))]
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
pub enum Network {
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

impl From<Network> for NetworkRs {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => Self::Mainnet,
            Network::Testnet3 => Self::Testnet3,
            Network::Testnet4 => Self::Testnet4,
            Network::Signet => Self::Signet,
            Network::Regtest => Self::Regtest,
        }
    }
}

impl From<NetworkRs> for Network {
    fn from(network: NetworkRs) -> Self {
        match network {
            NetworkRs::Mainnet => Self::Mainnet,
            NetworkRs::Testnet3 => Self::Testnet3,
            NetworkRs::Testnet4 => Self::Testnet4,
            NetworkRs::Signet => Self::Signet,
            NetworkRs::Regtest => Self::Regtest,
        }
    }
}

/// Configuration for a wallet environment.
#[derive(uniffi::Object)]
pub struct WalletConfig {
    deploy_env: DeployEnv,
    network: Network,
    use_sgx: bool,
    gateway_url: Option<String>,
}

#[uniffi::export]
impl WalletConfig {
    /// Create config for Bitcoin mainnet.
    #[uniffi::constructor]
    pub fn mainnet() -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Prod,
            network: Network::Mainnet,
            use_sgx: true,
            gateway_url: None,
        })
    }

    /// Create config for Bitcoin testnet3.
    #[uniffi::constructor]
    pub fn testnet3() -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Staging,
            network: Network::Testnet3,
            use_sgx: true,
            gateway_url: None,
        })
    }

    /// Create config for local development (regtest).
    #[uniffi::constructor(default(use_sgx = false, gateway_url = None))]
    pub fn regtest(use_sgx: bool, gateway_url: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            deploy_env: DeployEnv::Dev,
            network: Network::Regtest,
            use_sgx,
            gateway_url,
        })
    }

    /// Get the configured deployment environment.
    pub fn deploy_env(&self) -> DeployEnv {
        self.deploy_env.clone()
    }

    /// Get the configured Bitcoin network.
    pub fn network(&self) -> Network {
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
    ///
    /// `lexe_data_dir` defaults to `~/.lexe` if not specified.
    #[uniffi::method(default(lexe_data_dir = None))]
    pub fn seedphrase_path(
        &self,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<String> {
        let dir = lexe_data_dir.map(Ok).unwrap_or_else(|| {
            lexe::default_lexe_data_dir()
                .map(|p| p.to_string_lossy().into_owned())
        })?;
        let path = self
            .to_rs()
            .seedphrase_path(dir.as_ref())
            .to_string_lossy()
            .into_owned();
        Ok(path)
    }
}

impl WalletConfig {
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
        env_config: Arc<WalletConfig>,
    ) -> Result<Arc<Self>, SeedFileError> {
        let wallet_env = env_config.to_rs().wallet_env;
        let sdk = match SdkRootSeed::read(&wallet_env) {
            Ok(Some(sdk)) => sdk,
            Ok(None) => {
                let data_dir = default_lexe_data_dir().unwrap_or_default();
                let path = env_config
                    .seedphrase_path(Some(data_dir))
                    .unwrap_or_default();
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
        env_config: Arc<WalletConfig>,
    ) -> Result<(), SeedFileError> {
        let wallet_env = env_config.to_rs().wallet_env;
        self.as_sdk().write(&wallet_env).map_err(|e| {
            // Check if the root cause is an "already exists" IO error.
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::AlreadyExists
                {
                    let data_dir = default_lexe_data_dir().unwrap_or_default();
                    let path = env_config
                        .seedphrase_path(Some(data_dir))
                        .unwrap_or_default();
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
        let mnemonic = Mnemonic::from_str(&mnemonic)
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
    pub fn export_string(&self) -> String {
        self.sdk.export_string()
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
    inner: SdkLexeWallet,
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkLexeWallet::fresh(
            env_config.to_rs(),
            credentials.as_sdk(),
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let maybe_wallet = SdkLexeWallet::load(
            env_config.to_rs(),
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_wallet {
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkLexeWallet::load_or_fresh(
            env_config.to_rs(),
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;
        Ok(Arc::new(Self { inner }))
    }

    /// Create a wallet without local persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations (`sync_payments`, `list_payments`,
    /// `clear_payments`) are not available and will return an error if called.
    #[uniffi::constructor]
    pub fn without_db(
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkLexeWallet::without_db(
            env_config.to_rs(),
            credentials.as_sdk(),
        )?;
        Ok(Arc::new(Self { inner }))
    }

    // --- Getters --- //

    /// Get the user's hex-encoded public key.
    pub fn user_pk(&self) -> String {
        self.inner.user_config().user_pk.to_string()
    }

    // --- Node management --- //

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
    #[uniffi::method(default(partner_pk = None))]
    pub async fn signup(
        &self,
        root_seed: Arc<RootSeed>,
        partner_pk: Option<String>,
    ) -> FfiResult<()> {
        let partner = partner_pk
            .as_deref()
            .map(UserPk::from_str)
            .transpose()
            .context("Invalid partner user_pk")?;

        self.inner.signup(root_seed.as_sdk(), partner).await?;
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the node is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub async fn provision(
        &self,
        credentials: Arc<Credentials>,
    ) -> FfiResult<()> {
        self.inner.provision(credentials.as_sdk()).await?;
        Ok(())
    }

    /// Get information about the node.
    pub async fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = self.inner.node_info().await?;
        Ok(info.into())
    }

    // --- Paying and receiving Bitcoin --- //

    /// Analyze a Bitcoin or Lightning payment string.
    ///
    /// Returns a list of payment methods found (as `AnalyzeResponse`), sorted
    /// from most to least recommended. Each `PayableDetails` entry includes the
    /// payable string, method type ("invoice", "offer", "onchain", or "lnurl"),
    /// amount constraints, description, and expiration.
    ///
    /// Supported encodings:
    /// - BIP 321 URI: `bitcoin:bc1...`
    /// - Lightning URI: `lightning:ln...`
    /// - BOLT 11 invoice: `lnbc1...`
    /// - BOLT 12 offer: `lno1...`
    /// - Onchain bitcoin address: `bc1...`
    /// - Human Bitcoin Address: `₿satoshi@lexe.app`
    /// - Lightning Address: `satoshi@lexe.app`
    /// - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// Within the encodings, the following payment methods are supported:
    /// - BOLT 11 invoice
    /// - BOLT 12 offer
    /// - Bitcoin address
    /// - Lightning Address
    /// - LNURL
    ///
    /// `payable` is the string to analyze.
    #[uniffi::method]
    pub async fn analyze(&self, payable: String) -> FfiResult<AnalyzeResponse> {
        let req = SdkAnalyzeRequest {
            payment_string: payable,
        };
        let resp = self.inner.analyze(req).await?;
        Ok(resp.into())
    }

    /// Pay any string which encodes a Bitcoin or Lightning payment method.
    ///
    /// If multiple payment methods are encoded, the best recommended one is
    /// chosen. For finer control, use `analyze` first, then call
    /// `pay_invoice`, `pay_offer`, etc.
    ///
    /// Supported encodings:
    /// - BIP 321 URI: `bitcoin:bc1...`
    /// - Lightning URI: `lightning:ln...`
    /// - BOLT 11 invoice: `lnbc1...`
    /// - BOLT 12 offer: `lno1...`
    /// - Onchain bitcoin address: `bc1...`
    /// - Human Bitcoin Address: `₿satoshi@lexe.app`
    /// - Lightning Address: `satoshi@lexe.app`
    /// - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// `payable` is the string to pay.
    /// `amount_sats` is required when the payable has no encoded amount. If
    /// both specify an amount, they must match. For LNURL payables, the
    /// amount must be within the receiver's [min_amount, max_amount] range.
    /// `message` is an optional message to the recipient (BOLT12, LNURL).
    /// `personal_note` is an optional personal note (not visible to recipient).
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed). Exception: onchain sends return immediately with
    /// the payment still in `Pending` state, since on-chain confirmation takes
    /// ~1 hour.
    #[uniffi::method(default(
        amount_sats = None,
        message = None,
        personal_note = None,
    ))]
    pub async fn pay(
        &self,
        payable: String,
        amount_sats: Option<u64>,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow!("Invalid amount: {e}"))?;
        let req = SdkPayRequest {
            payable,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay(req).await?;
        Ok(resp.into())
    }

    /// Create a BOLT11 invoice.
    /// `expiration_secs` is the optional invoice expiry, in seconds;
    /// if `None`, the invoice expiry defaults to 86,400 (1 day).
    /// `amount_sats` is optional; if `None`, the invoice is amountless.
    /// `description` is shown to the payer, if provided.
    /// `personal_note` is a private note that the payer does not see.
    /// If provided, `personal_note` must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    /// `partner_pk` is the partner's user_pk for partner-set fees; must be set
    /// in order for the other partner fee fields to take effect.
    /// `partner_prop_fee_ppm` is the partner proportional fee in ppm; must be
    /// set if `partner_pk` is set.
    /// `partner_base_fee_sats` is the partner base fee in satoshis. If this is
    /// set, the invoice `amount_sats` must also be set.
    #[uniffi::method(default(
        expiration_secs = None,
        amount_sats = None,
        description = None,
        personal_note = None,
        partner_pk = None,
        partner_prop_fee_ppm = None,
        partner_base_fee_sats = None,
    ))]
    pub async fn create_invoice(
        &self,
        expiration_secs: Option<u32>,
        amount_sats: Option<u64>,
        description: Option<String>,
        personal_note: Option<String>,
        partner_pk: Option<String>,
        partner_prop_fee_ppm: Option<i32>,
        partner_base_fee_sats: Option<u64>,
    ) -> FfiResult<CreateInvoiceResponse> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid amount")?;

        let partner_pk = partner_pk
            .as_deref()
            .map(UserPk::from_str)
            .transpose()
            .context("Invalid partner_pk")?;

        let partner_prop_fee = partner_prop_fee_ppm.map(Ppm::new);

        let partner_base_fee = partner_base_fee_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid partner_base_fee")?;

        let req = SdkCreateInvoiceRequest {
            expiration_secs,
            amount,
            description,
            personal_note,
            partner_pk,
            partner_prop_fee,
            partner_base_fee,
        };
        let resp = self.inner.create_invoice(req).await?;
        Ok(resp.into())
    }

    /// Pay a BOLT11 invoice.
    /// `fallback_amount_sats` is required if the invoice is amountless.
    /// `personal_note` is a private note that the receiver does not see.
    /// If provided, `personal_note` must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(
        fallback_amount_sats = None,
        personal_note = None,
    ))]
    pub async fn pay_invoice(
        &self,
        invoice: String,
        fallback_amount_sats: Option<u64>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let invoice =
            InvoiceRs::from_str(&invoice).context("Invalid invoice")?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid fallback amount")?;

        let req = SdkPayInvoiceRequest {
            invoice,
            fallback_amount,
            personal_note,
        };
        let resp = self.inner.pay_invoice(req).await?;
        Ok(resp.into())
    }

    /// Create a BOLT 12 offer to receive Lightning payments.
    ///
    /// Unlike invoices, offers are reusable: multiple payments can be made to
    /// it, including from multiple payers.
    ///
    /// `description` is shown to the sender when they scan the offer. If
    /// provided, it must be non-empty and no longer than 200 chars / 512
    /// UTF-8 bytes.
    /// `min_amount_sats` is an optional minimum payment size.
    /// `expiration_secs` is an optional expiration time in seconds from now.
    #[uniffi::method(default(
        description = None,
        min_amount_sats = None,
        expiration_secs = None,
    ))]
    pub async fn create_offer(
        &self,
        description: Option<String>,
        min_amount_sats: Option<u64>,
        expiration_secs: Option<u32>,
    ) -> FfiResult<CreateOfferResponse> {
        let min_amount = min_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid min_amount")?;

        let req = SdkCreateOfferRequest {
            description,
            min_amount,
            expiration_secs,
        };
        let resp = self.inner.create_offer(req).await?;
        Ok(resp.into())
    }

    /// Pay a BOLT 12 offer over Lightning.
    ///
    /// `offer` is the BOLT 12 offer string to pay.
    /// `amount_sats` is the amount to pay in satoshis.
    /// `message` is a note visible to the receiver. If provided, it must
    /// be non-empty and no longer than 200 chars / 512 UTF-8 bytes.
    /// `personal_note` is a private note that the receiver does not see. If
    /// provided, it must be non-empty and no longer than 200 chars / 512 UTF-8
    /// bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(message = None, personal_note = None))]
    pub async fn pay_offer(
        &self,
        offer: String,
        amount_sats: u64,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let offer = OfferRs::from_str(&offer).context("Invalid offer")?;
        let amount = AmountRs::try_from_sats_u64(amount_sats)
            .context("Invalid amount")?;

        let req = SdkPayOfferRequest {
            offer,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay_offer(req).await?;
        Ok(resp.into())
    }

    /// Pay an LNURL via the `payRequest` flow.
    ///
    /// Use `analyze` to get the associated LNURL pay request, which contains
    /// information on amount constraints, message limits, and more.
    ///
    /// `lnurl` is the LNURL string to pay to.
    /// `amount_sats` is the amount to pay in satoshis. If the LNURL endpoint
    /// specifies a minimum or maximum amount, this value must satisfy those
    /// limits.
    /// `message` is a note visible to the recipient. It is only sent if the
    /// LNURL endpoint supports it, and is truncated to the endpoint's length
    /// limit if needed.
    /// `personal_note` is a private note that the receiver does not see. If
    /// provided, it must be non-empty and no longer than 200 chars / 512 UTF-8
    /// bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(message = None, personal_note = None))]
    pub async fn pay_lnurl(
        &self,
        lnurl: String,
        amount_sats: u64,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = AmountRs::try_from_sats_u64(amount_sats)
            .context("Invalid amount")?;

        let req = SdkPayLnurlRequest {
            lnurl: Some(lnurl),
            pay_request: None,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay_lnurl(req).await?;
        Ok(resp.into())
    }

    /// Withdraw an LNURL via the `withdrawRequest` flow.
    ///
    /// Use `analyze` to get the associated LNURL withdraw request, which
    /// contains information on amount constraints, default description,
    /// and more.
    ///
    /// `lnurl` is the LNURL string to withdraw from.
    /// `amount_sats` is the amount to withdraw in satoshis. It must satisfy the
    /// minimum and maximum limits set by the LNURL endpoint. If `None`, the
    /// maximum amount is withdrawn.
    /// `description` is encoded into the withdrawal invoice and visible to the
    /// LNURL endpoint. If `None`, the description specified by the LNURL
    /// endpoint (if any) is used.
    /// `personal_note` is a private note that the LNURL endpoint does not see.
    /// If provided, it must be non-empty and no longer than 200 chars / 512
    /// UTF-8 bytes.
    ///
    /// Returns the resulting `Payment` once the withdrawal reaches a terminal
    /// state (completed or failed).
    #[uniffi::method(default(
        amount_sats = None,
        description = None,
        personal_note = None
    ))]
    pub async fn withdraw_lnurl(
        &self,
        lnurl: String,
        amount_sats: Option<u64>,
        description: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid amount")?;

        let req = SdkWithdrawLnurlRequest {
            lnurl: Some(lnurl),
            withdraw_request: None,
            amount,
            description,
            personal_note,
        };
        let resp = self.inner.withdraw_lnurl(req).await?;
        Ok(resp.into())
    }

    // --- Payment information and management --- //

    /// Get a payment by its `index` string.
    pub async fn get_payment(
        &self,
        index: String,
    ) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkGetPaymentRequest { index };
        let resp = self.inner.get_payment(req).await?;
        Ok(resp.payment.map(Into::into))
    }

    /// Get a batch of payments in ascending `updated_at` order, starting
    /// from a given `updated_at` index.
    ///
    /// `start_index` is the cursor at which the results should start,
    /// exclusive. If `None`, the least recently updated payments will be
    /// returned first. `limit` caps the number of payments returned
    /// (max 100, default 50).
    #[uniffi::method(default(start_index = None, limit = None))]
    pub async fn get_updated_payments(
        &self,
        start_index: Option<String>,
        limit: Option<u16>,
    ) -> FfiResult<GetUpdatedPaymentsResponse> {
        let start_index = start_index
            .map(|s| PaymentUpdatedIndexRs::from_str(&s))
            .transpose()?;
        let req = SdkGetUpdatedPaymentsRequest { start_index, limit };
        let resp = self.inner.get_updated_payments(req).await?;
        Ok(GetUpdatedPaymentsResponse::from(resp))
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 600 seconds (10 minutes).
    /// Maximum timeout is 86,400 seconds (24 hours).
    #[uniffi::method(default(timeout_secs = None))]
    pub async fn wait_for_payment(
        &self,
        index: String,
        timeout_secs: Option<u32>,
    ) -> FfiResult<Payment> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let timeout = timeout_secs.map(|secs| Duration::from_secs(secs.into()));
        let payment = self.inner.wait_for_payment(index, timeout).await?;
        Ok(Payment::from(payment))
    }

    /// Update a payment's personal note.
    /// Call `sync_payments` first so the payment exists locally.
    /// If `personal_note` is `Some`, it must be non-empty and at most 200 chars
    /// / 512 UTF-8 bytes.
    pub async fn update_personal_note(
        &self,
        index: String,
        personal_note: Option<String>,
    ) -> FfiResult<()> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = UpdatePersonalNoteRequest {
            index,
            personal_note,
        };
        self.inner.update_personal_note(req).await?;
        Ok(())
    }

    /// Sync payments from the user node to the local payments cache.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
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
        )?;

        Ok(ListPaymentsResponse {
            payments: resp.payments.into_iter().map(Payment::from).collect(),
            next_index: resp.next_index.map(|idx| idx.to_string()),
        })
    }

    /// Clear all locally cached payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call `sync_payments` to re-populate.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.inner.clear_payments()?;
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
    inner: SdkBlockingLexeWallet,
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkBlockingLexeWallet::fresh(
            env_config.to_rs(),
            credentials.as_sdk(),
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> Result<Arc<Self>, LoadWalletError> {
        let maybe_wallet = SdkBlockingLexeWallet::load(
            env_config.to_rs(),
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )
        .map_err(|e| LoadWalletError::LoadFailed {
            message: format!("{e:#}"),
        })?;

        match maybe_wallet {
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
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkBlockingLexeWallet::load_or_fresh(
            env_config.to_rs(),
            credentials.as_sdk(),
            lexe_data_dir.map(PathBuf::from),
        )?;
        Ok(Arc::new(Self { inner }))
    }

    /// Create a wallet without local persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations (`sync_payments`, `list_payments`,
    /// `clear_payments`) are not available and will return an error if called.
    #[uniffi::constructor]
    pub fn without_db(
        env_config: Arc<WalletConfig>,
        credentials: Arc<Credentials>,
    ) -> FfiResult<Arc<Self>> {
        let inner = SdkBlockingLexeWallet::without_db(
            env_config.to_rs(),
            credentials.as_sdk(),
        )?;
        Ok(Arc::new(Self { inner }))
    }

    // --- Client accessors --- //

    /// Get the user's hex-encoded public key.
    pub fn user_pk(&self) -> String {
        self.inner.user_config().user_pk.to_string()
    }

    // --- Node management --- //

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
    #[uniffi::method(default(partner_pk = None))]
    pub fn signup(
        &self,
        root_seed: Arc<RootSeed>,
        partner_pk: Option<String>,
    ) -> FfiResult<()> {
        let partner = partner_pk
            .as_deref()
            .map(UserPk::from_str)
            .transpose()
            .context("Invalid partner user_pk")?;

        self.inner.signup(root_seed.as_sdk(), partner)?;
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the node is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub fn provision(&self, credentials: Arc<Credentials>) -> FfiResult<()> {
        self.inner.provision(credentials.as_sdk())?;
        Ok(())
    }

    /// Get information about the node.
    pub fn node_info(&self) -> FfiResult<NodeInfo> {
        let info = self.inner.node_info()?;
        Ok(info.into())
    }

    // --- Paying and receiving Bitcoin --- //

    /// Analyze a Bitcoin or Lightning payment string.
    ///
    /// Returns a list of payment methods found (as `AnalyzeResponse`), sorted
    /// from most to least recommended. Each `PayableDetails` entry includes the
    /// payable string, method type ("invoice", "offer", "onchain", or "lnurl"),
    /// amount constraints, description, and expiration.
    ///
    /// Supported encodings:
    /// - BIP 321 URI: `bitcoin:bc1...`
    /// - Lightning URI: `lightning:ln...`
    /// - BOLT 11 invoice: `lnbc1...`
    /// - BOLT 12 offer: `lno1...`
    /// - Onchain bitcoin address: `bc1...`
    /// - Human Bitcoin Address: `₿satoshi@lexe.app`
    /// - Lightning Address: `satoshi@lexe.app`
    /// - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// Within the encodings, the following payment methods are supported:
    /// - BOLT 11 invoice
    /// - BOLT 12 offer
    /// - Bitcoin address
    /// - Lightning Address
    /// - LNURL
    ///
    /// `payable` is the string to analyze.
    #[uniffi::method]
    pub fn analyze(&self, payable: String) -> FfiResult<AnalyzeResponse> {
        let req = SdkAnalyzeRequest {
            payment_string: payable,
        };
        let resp = self.inner.analyze(req)?;
        Ok(resp.into())
    }

    /// Pay any string which encodes a Bitcoin or Lightning payment method.
    ///
    /// If multiple payment methods are encoded, the best recommended one is
    /// chosen. For finer control, use `analyze` first, then call
    /// `pay_invoice`, `pay_offer`, etc.
    ///
    /// Supported encodings:
    /// - BIP 321 URI: `bitcoin:bc1...`
    /// - Lightning URI: `lightning:ln...`
    /// - BOLT 11 invoice: `lnbc1...`
    /// - BOLT 12 offer: `lno1...`
    /// - Onchain bitcoin address: `bc1...`
    /// - Human Bitcoin Address: `₿satoshi@lexe.app`
    /// - Lightning Address: `satoshi@lexe.app`
    /// - LNURL: `lnurl1...` or `lnurlp://domain.com/path`
    ///
    /// `payable` is the string to pay.
    /// `amount_sats` is required when the payable has no encoded amount. If
    /// both specify an amount, they must match. For LNURL payables, the
    /// amount must be within the receiver's [min_amount, max_amount] range.
    /// `message` is an optional message to the recipient (BOLT12, LNURL).
    /// `personal_note` is an optional personal note (not visible to recipient).
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed). Exception: onchain sends return immediately with
    /// the payment still in `Pending` state, since on-chain confirmation takes
    /// ~1 hour.
    #[uniffi::method(default(
        amount_sats = None,
        message = None,
        personal_note = None,
    ))]
    pub fn pay(
        &self,
        payable: String,
        amount_sats: Option<u64>,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow!("Invalid amount: {e}"))?;
        let req = SdkPayRequest {
            payable,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay(req)?;
        Ok(resp.into())
    }

    /// Create a BOLT11 invoice.
    /// `expiration_secs` is the optional invoice expiry, in seconds;
    /// if `None`, the invoice expiry defaults to 86,400 (1 day).
    /// `amount_sats` is optional; if `None`, the invoice is amountless.
    /// `description` is shown to the payer, if provided.
    /// `personal_note` is a private note that the payer does not see.
    /// If provided, `personal_note` must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    /// `partner_pk` is the partner's user_pk for partner-set fees; must be set
    /// in order for the other partner fee fields to take effect.
    /// `partner_prop_fee_ppm` is the partner proportional fee in ppm; must be
    /// set if `partner_pk` is set.
    /// `partner_base_fee_sats` is the partner base fee in satoshis. If this is
    /// set, the invoice `amount_sats` must also be set.
    #[uniffi::method(default(
        expiration_secs = None,
        amount_sats = None,
        description = None,
        personal_note = None,
        partner_pk = None,
        partner_prop_fee_ppm = None,
        partner_base_fee_sats = None,
    ))]
    pub fn create_invoice(
        &self,
        expiration_secs: Option<u32>,
        amount_sats: Option<u64>,
        description: Option<String>,
        personal_note: Option<String>,
        partner_pk: Option<String>,
        partner_prop_fee_ppm: Option<i32>,
        partner_base_fee_sats: Option<u64>,
    ) -> FfiResult<CreateInvoiceResponse> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid amount")?;

        let partner_pk = partner_pk
            .as_deref()
            .map(UserPk::from_str)
            .transpose()
            .context("Invalid partner_pk")?;

        let partner_prop_fee = partner_prop_fee_ppm.map(Ppm::new);

        let partner_base_fee = partner_base_fee_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid partner_base_fee")?;

        let req = SdkCreateInvoiceRequest {
            expiration_secs,
            amount,
            description,
            personal_note,
            partner_pk,
            partner_prop_fee,
            partner_base_fee,
        };
        let resp = self.inner.create_invoice(req)?;
        Ok(resp.into())
    }

    /// Pay a BOLT11 invoice.
    /// `fallback_amount_sats` is required if the invoice is amountless.
    /// `personal_note` is a private note that the receiver does not see.
    /// If provided, `personal_note` must be non-empty and at most 200 chars /
    /// 512 UTF-8 bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(
        fallback_amount_sats = None,
        personal_note = None,
    ))]
    pub fn pay_invoice(
        &self,
        invoice: String,
        fallback_amount_sats: Option<u64>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let invoice =
            InvoiceRs::from_str(&invoice).context("Invalid invoice")?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid fallback amount")?;

        let req = SdkPayInvoiceRequest {
            invoice,
            fallback_amount,
            personal_note,
        };
        let resp = self.inner.pay_invoice(req)?;
        Ok(resp.into())
    }

    /// Create a BOLT 12 offer to receive Lightning payments.
    ///
    /// Unlike invoices, offers are reusable: multiple payments can be made to
    /// it, including from multiple payers.
    ///
    /// `description` is shown to the sender when they scan the offer. If
    /// provided, it must be non-empty and no longer than 200 chars / 512
    /// UTF-8 bytes.
    /// `min_amount_sats` is an optional minimum payment size.
    /// `expiration_secs` is an optional expiration time in seconds from now.
    #[uniffi::method(default(
        description = None,
        min_amount_sats = None,
        expiration_secs = None,
    ))]
    pub fn create_offer(
        &self,
        description: Option<String>,
        min_amount_sats: Option<u64>,
        expiration_secs: Option<u32>,
    ) -> FfiResult<CreateOfferResponse> {
        let min_amount = min_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid min_amount")?;

        let req = SdkCreateOfferRequest {
            description,
            min_amount,
            expiration_secs,
        };
        let resp = self.inner.create_offer(req)?;
        Ok(resp.into())
    }

    /// Pay a BOLT 12 offer over Lightning.
    ///
    /// `offer` is the BOLT 12 offer string to pay.
    /// `amount_sats` is the amount to pay in satoshis.
    /// `message` is a note visible to the receiver. If provided, it must
    /// be non-empty and no longer than 200 chars / 512 UTF-8 bytes.
    /// `personal_note` is a private note that the receiver does not see. If
    /// provided, it must be non-empty and no longer than 200 chars / 512 UTF-8
    /// bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(message = None, personal_note = None))]
    pub fn pay_offer(
        &self,
        offer: String,
        amount_sats: u64,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let offer = OfferRs::from_str(&offer).context("Invalid offer")?;
        let amount = AmountRs::try_from_sats_u64(amount_sats)
            .context("Invalid amount")?;

        let req = SdkPayOfferRequest {
            offer,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay_offer(req)?;
        Ok(resp.into())
    }

    /// Pay an LNURL via the `payRequest` flow.
    ///
    /// Use `analyze` to get the associated LNURL pay request, which contains
    /// information on amount constraints, message limits, and more.
    ///
    /// `lnurl` is the LNURL string to pay to.
    /// `amount_sats` is the amount to pay in satoshis. If the LNURL endpoint
    /// specifies a minimum or maximum amount, this value must satisfy those
    /// limits.
    /// `message` is a note visible to the recipient. It is only sent if the
    /// LNURL endpoint supports it, and is truncated to the endpoint's length
    /// limit if needed.
    /// `personal_note` is a private note that the receiver does not see. If
    /// provided, it must be non-empty and no longer than 200 chars / 512 UTF-8
    /// bytes.
    ///
    /// Returns the resulting `Payment` once it reaches a terminal state
    /// (completed or failed).
    #[uniffi::method(default(message = None, personal_note = None))]
    pub fn pay_lnurl(
        &self,
        lnurl: String,
        amount_sats: u64,
        message: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = AmountRs::try_from_sats_u64(amount_sats)
            .context("Invalid amount")?;

        let req = SdkPayLnurlRequest {
            lnurl: Some(lnurl),
            pay_request: None,
            amount,
            message,
            personal_note,
        };
        let resp = self.inner.pay_lnurl(req)?;
        Ok(resp.into())
    }

    /// Withdraw an LNURL via the `withdrawRequest` flow.
    ///
    /// Use `analyze` to get the associated LNURL withdraw request, which
    /// contains information on amount constraints, default description,
    /// and more.
    ///
    /// `lnurl` is the LNURL string to withdraw from.
    /// `amount_sats` is the amount to withdraw in satoshis. It must satisfy the
    /// minimum and maximum limits set by the LNURL endpoint. If `None`, the
    /// maximum amount is withdrawn.
    /// `description` is encoded into the withdrawal invoice and visible to the
    /// LNURL endpoint. If `None`, the description specified by the LNURL
    /// endpoint (if any) is used.
    /// `personal_note` is a private note that the LNURL endpoint does not see.
    /// If provided, it must be non-empty and no longer than 200 chars / 512
    /// UTF-8 bytes.
    ///
    /// Returns the resulting `Payment` once the withdrawal reaches a terminal
    /// state (completed or failed).
    #[uniffi::method(default(
        amount_sats = None,
        description = None,
        personal_note = None
    ))]
    pub fn withdraw_lnurl(
        &self,
        lnurl: String,
        amount_sats: Option<u64>,
        description: Option<String>,
        personal_note: Option<String>,
    ) -> FfiResult<Payment> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .context("Invalid amount")?;

        let req = SdkWithdrawLnurlRequest {
            lnurl: Some(lnurl),
            withdraw_request: None,
            amount,
            description,
            personal_note,
        };
        let resp = self.inner.withdraw_lnurl(req)?;
        Ok(resp.into())
    }

    // --- Payment information and management --- //

    /// Get a payment by its `index` string.
    pub fn get_payment(&self, index: String) -> FfiResult<Option<Payment>> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = SdkGetPaymentRequest { index };
        let resp = self.inner.get_payment(req)?;
        Ok(resp.payment.map(Into::into))
    }

    /// Get a batch of payments in ascending `updated_at` order, starting
    /// from a given `updated_at` index.
    ///
    /// `start_index` is the cursor at which the results should start,
    /// exclusive. If `None`, the least recently updated payments will be
    /// returned first. `limit` caps the number of payments returned
    /// (max 100, default 50).
    #[uniffi::method(default(start_index = None, limit = None))]
    pub fn get_updated_payments(
        &self,
        start_index: Option<String>,
        limit: Option<u16>,
    ) -> FfiResult<GetUpdatedPaymentsResponse> {
        let start_index = start_index
            .map(|s| PaymentUpdatedIndexRs::from_str(&s))
            .transpose()?;
        let req = SdkGetUpdatedPaymentsRequest { start_index, limit };
        let resp = self.inner.get_updated_payments(req)?;
        Ok(GetUpdatedPaymentsResponse::from(resp))
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 600 seconds (10 minutes).
    /// Maximum timeout is 86,400 seconds (24 hours).
    #[uniffi::method(default(timeout_secs = None))]
    pub fn wait_for_payment(
        &self,
        index: String,
        timeout_secs: Option<u32>,
    ) -> FfiResult<Payment> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let timeout = timeout_secs.map(|secs| Duration::from_secs(secs.into()));
        let payment = self.inner.wait_for_payment(index, timeout)?;
        Ok(Payment::from(payment))
    }

    /// Update a payment's personal note.
    /// Call `sync_payments` first so the payment exists locally.
    /// If `personal_note` is `Some`, it must be non-empty and at most 200 chars
    /// / 512 UTF-8 bytes.
    pub fn update_personal_note(
        &self,
        index: String,
        personal_note: Option<String>,
    ) -> FfiResult<()> {
        let index = PaymentCreatedIndexRs::from_str(&index)?;
        let req = UpdatePersonalNoteRequest {
            index,
            personal_note,
        };
        self.inner.update_personal_note(req)?;
        Ok(())
    }

    /// Sync payments from the user node to the local payments cache.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
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
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
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
        )?;

        Ok(ListPaymentsResponse {
            payments: resp.payments.into_iter().map(Payment::from).collect(),
            next_index: resp.next_index.map(|idx| idx.to_string()),
        })
    }

    /// Clear all locally cached payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call `sync_payments` to re-populate.
    ///
    /// Requires a wallet created with `fresh`, `load`, or `load_or_fresh`.
    /// Returns an error if local persistence is disabled for this wallet.
    pub fn clear_payments(&self) -> FfiResult<()> {
        self.inner.clear_payments()?;
        Ok(())
    }
}

// ================ //
// --- Payments --- //
// ================ //

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
///
/// Its primary purpose is to prevent accidental double payments.
#[derive(uniffi::Object)]
pub struct ClientPaymentId {
    inner: ClientPaymentIdRs,
}

#[uniffi::export]
impl ClientPaymentId {
    /// Generate a random [`ClientPaymentId`].
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self {
            inner: ClientPaymentIdRs::generate(),
        })
    }

    /// Return the 32-byte id.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.0.to_vec()
    }

    /// Encode the id as a 64-character hex string.
    pub fn to_hex(&self) -> String {
        self.inner.to_string()
    }
}

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

/// Information about a payment.
#[derive(Clone, uniffi::Record)]
pub struct Payment {
    /// Unique payment identifier, ordered by `created_at_ms`.
    /// Format: `<created_at_ms>-<payment_id>`.
    pub index: String,

    /// The technical 'rail' used to fulfill a payment:
    /// 'onchain', 'invoice', 'offer', 'spontaneous', 'waived_fee', etc.
    pub rail: PaymentRail,

    /// Application-level payment kind.
    pub kind: PaymentKind,

    /// The payment direction: `"inbound"`, `"outbound"`, or `"info"`.
    pub direction: PaymentDirection,

    /// (Lightning payments only) Hex-encoded payment hash.
    pub hash: Option<String>,

    /// (Lightning payments only) Hex-encoded payment preimage. Serves as
    /// proof-of-payment for outbound payments. For inbound payments, only
    /// populated if the payment succeeded.
    pub preimage: Option<String>,

    /// (Offer payments only) Hex-encoded id of the BOLT12 offer used in this
    /// payment.
    pub offer_id: Option<String>,

    /// (Onchain payments only) Hex-encoded Bitcoin txid.
    pub txid: Option<String>,

    /// The amount of this payment, in satoshis.
    ///
    /// - If this is a completed inbound invoice payment, this is the amount we
    ///   received.
    /// - If this is a pending or failed inbound invoice payment, this is the
    ///   amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always included.
    pub amount_sats: Option<u64>,

    /// The fees for this payment, in satoshis.
    ///
    /// If `partner_pk` is set, this means that the partner, not Lexe,
    /// determined the fee for this payment.
    pub fees_sats: u64,

    /// Hex-encoded partner user_pk, if the fees for this payment were set by
    /// a Lexe partner, instead of using Lexe's default fees.
    pub partner_pk: Option<String>,

    /// The proportional fee set by the partner, in parts per million (ppm).
    pub partner_prop_fee_ppm: Option<u32>,

    /// The base fee set by the partner, in satoshis.
    pub partner_base_fee_sats: Option<u64>,

    /// The status of this payment: "pending", "completed", or "failed".
    pub status: PaymentStatus,

    /// The payment status as a human-readable message. These strings are
    /// customized per payment type, e.g. "invoice generated", "timed out".
    pub status_msg: String,

    /// (Onchain send only) The address that we're sending to.
    pub address: Option<String>,

    /// (Invoice payments only) The BOLT 11 invoice used in this payment.
    pub invoice: Option<Invoice>,

    /// (Offer payments only) The payer's self-reported human-readable name.
    pub payer_name: Option<String>,

    /// (Offer payments, LNURL-pay invoices) A payer-provided message for this
    /// payment.
    pub message: Option<String>,

    /// An optional personal note which a user can attach to any payment.
    /// A personal note can always be added or modified when a payment already
    /// exists, but this may not always be possible at creation time.
    pub personal_note: Option<String>,

    /// (Onchain send only) The confirmation priority used for this payment.
    pub priority: Option<ConfirmationPriority>,

    /// The invoice or offer expiry time, in milliseconds since the UNIX epoch.
    /// `None` otherwise, or if the timestamp overflows.
    pub expires_at_ms: Option<u64>,

    /// If this payment is finalized, meaning it is "completed" or "failed",
    /// this is the time it was finalized, in milliseconds since the UNIX
    /// epoch.
    pub finalized_at_ms: Option<u64>,

    /// When this payment was created, in milliseconds since the UNIX epoch.
    pub created_at_ms: u64,

    /// When this payment was last updated, in milliseconds since the UNIX
    /// epoch.
    pub updated_at_ms: u64,
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
            hash,
            preimage,
            offer_id,
            txid,
            amount,
            fees,
            partner_pk,
            partner_prop_fee,
            partner_base_fee,
            status,
            status_msg,
            address,
            invoice,
            tx: _,
            payer_name,
            message,
            personal_note,
            priority,
            expires_at,
            finalized_at,
            created_at,
            updated_at,
        } = payment;

        Self {
            index: index.to_string(),
            rail: rail.into(),
            kind: kind.into(),
            direction: direction.into(),
            hash: hash.map(|h| h.to_hex()),
            preimage: preimage.map(|p| p.to_hex()),
            offer_id: offer_id.map(|o| o.to_hex()),
            txid: txid.map(|t| t.to_string()),
            amount_sats: amount.map(|a| a.sats_u64()),
            fees_sats: fees.sats_u64(),
            partner_pk: partner_pk.map(|pk| pk.to_hex()),
            partner_prop_fee_ppm: partner_prop_fee.map(|p| p.to_u32()),
            partner_base_fee_sats: partner_base_fee.map(|a| a.sats_u64()),
            status: status.into(),
            status_msg,
            address: address
                .as_ref()
                .map(|a| a.assume_checked_ref().to_string()),
            invoice: invoice.map(|arc| Invoice::from(&*arc)),
            payer_name,
            message,
            personal_note,
            priority: priority.map(Into::into),
            expires_at_ms: expires_at.map(|t| t.to_millis()),
            finalized_at_ms: finalized_at.map(|t| t.to_millis()),
            created_at_ms: created_at.to_millis(),
            updated_at_ms: updated_at.to_millis(),
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

/// Response from getting updated payments.
#[derive(Clone, uniffi::Record)]
pub struct GetUpdatedPaymentsResponse {
    /// The updated payments which were fetched.
    pub payments: Vec<Payment>,
    /// Cursor for fetching the next page of updated payments.
    pub updated_index: Option<String>,
}

impl From<SdkGetUpdatedPaymentsResponse> for GetUpdatedPaymentsResponse {
    fn from(resp: SdkGetUpdatedPaymentsResponse) -> Self {
        Self {
            payments: resp.payments.into_iter().map(Payment::from).collect(),
            updated_index: resp.updated_index.map(|idx| idx.to_string()),
        }
    }
}

// =================== //
// --- Pay/Analyze --- //
// =================== //

/// A single "payment method" -- each variant corresponds with a single
/// linear (outbound) payment flow.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentMethod {
    /// An onchain Bitcoin payment.
    Onchain {
        /// The onchain Bitcoin address.
        address: String,
        /// The amount to pay to the onchain address, if specified.
        /// Parsed from a BIP321 URI or
        /// BOLT11 invoice containing the onchain address.
        amount_sats: Option<u64>,
        /// A label for the onchain address. Parsed from a BIP321 URI
        /// containing the onchain address.
        label: Option<String>,
        /// A message describing the transaction. Parsed from a BIP321 URI
        /// or BOLT11 invoice containing the onchain address.
        message: Option<String>,
    },
    /// A BOLT11 Lightning invoice payment.
    Invoice {
        /// The BOLT 11 invoice.
        invoice: Invoice,
    },
    /// A BOLT12 offer payment.
    Offer {
        /// The full BOLT12 offer.
        offer: Offer,
        /// Amount from a BIP321 URI which contained the offer, in satoshis.
        bip321_amount_sats: Option<u64>,
    },
    /// An LNURL-pay payment (LUD-06).
    LnurlPay {
        /// An LNURL-pay URI.
        lnurl: String,
        /// LNURL-pay request, which includes information about
        /// the amount constraints, callback, etc. associated with the LNURL.
        pay_request: LnurlPayRequest,
    },
}

impl From<SdkPaymentMethod> for PaymentMethod {
    fn from(method: SdkPaymentMethod) -> Self {
        match method {
            SdkPaymentMethod::Onchain {
                address,
                amount,
                label,
                message,
            } => Self::Onchain {
                address: address.to_string(),
                amount_sats: amount.map(|amt| amt.sats_u64()),
                label,
                message,
            },
            SdkPaymentMethod::Invoice { invoice } => Self::Invoice {
                invoice: Invoice::from(&invoice),
            },
            SdkPaymentMethod::Offer {
                offer,
                bip321_amount,
            } => Self::Offer {
                offer: Offer::from(offer),
                bip321_amount_sats: bip321_amount.map(|amt| amt.sats_u64()),
            },
            SdkPaymentMethod::LnurlPay { lnurl, pay_request } =>
                Self::LnurlPay {
                    lnurl,
                    pay_request: LnurlPayRequest::from(pay_request),
                },
        }
    }
}

// TODO(nicole): move structs to an // --- Lnurl --- // section when added
/// The validated and parsed LNURL-pay request (tagged "payRequest").
#[derive(Clone, uniffi::Record)]
pub struct LnurlPayRequest {
    /// Callback URL to request invoice from.
    pub callback: String,
    /// Minimum sendable amount, in satoshis.
    pub min_sendable_sats: u64,
    /// Maximum sendable amount, in satoshis.
    pub max_sendable_sats: u64,
    /// Parsed metadata with description and description hash.
    pub metadata: LnurlPayRequestMetadata,
    /// LUD-12: Max comment length in characters, if comments are supported.
    pub comment_allowed: Option<u16>,
}

/// The metadata inside an [`LnurlPayRequest`].
#[derive(Clone, uniffi::Record)]
pub struct LnurlPayRequestMetadata {
    /// Short description from `text/plain` (required, LUD-06).
    pub description: String,
    /// Long description from `text/long-desc` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub long_description: Option<String>,
    /// PNG thumbnail from `image/png;base64` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub image_png_base64: Option<String>,
    /// JPEG thumbnail from `image/jpeg;base64` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub image_jpeg_base64: Option<String>,
    /// Internet identifier from `text/identifier` (LUD-16).
    /// LNURL-Pay via LUD-16 requires this or `text/email` to be set.
    pub identifier: Option<String>,
    /// Email address from `text/email` (LUD-16).
    /// LNURL-Pay via LUD-16 requires this or `text/identifier` to be set.
    pub email: Option<String>,
    /// Hex-encoded SHA256 hash of raw metadata for invoice validation.
    pub description_hash: String,
    /// The original unparsed metadata string.
    pub raw: String,
}

impl From<SdkLnurlPayRequest> for LnurlPayRequest {
    fn from(req: SdkLnurlPayRequest) -> Self {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the conversion below.
        let SdkLnurlPayRequest {
            callback,
            min_sendable,
            max_sendable,
            metadata,
            comment_allowed,
        } = req;

        Self {
            callback,
            min_sendable_sats: min_sendable.sats_u64(),
            max_sendable_sats: max_sendable.sats_u64(),
            metadata: LnurlPayRequestMetadata::from(metadata),
            comment_allowed,
        }
    }
}

impl From<SdkLnurlPayRequestMetadata> for LnurlPayRequestMetadata {
    fn from(metadata: SdkLnurlPayRequestMetadata) -> Self {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the conversion below.
        let SdkLnurlPayRequestMetadata {
            description,
            long_description,
            image_png_base64,
            image_jpeg_base64,
            identifier,
            email,
            description_hash,
            raw,
        } = metadata;

        Self {
            description,
            long_description,
            image_png_base64,
            image_jpeg_base64,
            identifier,
            email,
            description_hash: lexe::util::hex::encode(&description_hash),
            raw,
        }
    }
}

/// Describes basic information for a payable string.
#[derive(Clone, uniffi::Record)]
pub struct PayableDetails {
    /// String encoding of the payable.
    pub payable: String,
    /// The deserialized payment method.
    pub method: PaymentMethod,

    /// Description encoded in the payable, if any.
    pub description: Option<String>,

    /// Amount encoded in the payable, in satoshis (if any).
    ///
    /// If no amount, the payer should specify an amount to pay.
    ///
    /// Won't be supplied if min_amount or max_amount are specified.
    pub amount_sats: Option<u64>,
    /// The minimum amount that can be paid to the payable.
    ///
    /// Won't be supplied if amount is specified.
    pub min_amount_sats: Option<u64>,
    /// Maximum amount that can be paid to the payable.
    ///
    /// Won't be supplied if amount is specified.
    pub max_amount_sats: Option<u64>,

    /// Payable expiration time (milliseconds since the UNIX epoch).
    pub expires_at_ms: Option<u64>,
}

impl From<SdkPayableDetails> for PayableDetails {
    fn from(resp: SdkPayableDetails) -> Self {
        Self {
            payable: resp.payable,
            method: PaymentMethod::from(resp.method),
            description: resp.description,
            amount_sats: resp.amount.map(|a| a.sats_u64()),
            min_amount_sats: resp.min_amount.map(|a| a.sats_u64()),
            max_amount_sats: resp.max_amount.map(|a| a.sats_u64()),
            expires_at_ms: resp.expires_at.map(|t| t.to_millis()),
        }
    }
}

/// Response from analyzing a payable string.
#[derive(Clone, uniffi::Record)]
pub struct AnalyzeResponse {
    /// Valid payment routes encoded in the analyzed string, sorted from most
    /// to least recommended.
    pub payables: Vec<PayableDetails>,
}

impl From<SdkAnalyzeResponse> for AnalyzeResponse {
    fn from(resp: SdkAnalyzeResponse) -> Self {
        let payables = resp
            .payables
            .into_iter()
            .map(PayableDetails::from)
            .collect();
        Self { payables }
    }
}

// ================ //
// --- Invoices --- //
// ================ //

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

impl From<&InvoiceRs> for Invoice {
    fn from(invoice: &InvoiceRs) -> Self {
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

// ============== //
// --- Offers --- //
// ============== //

/// A BOLT12 Lightning offer.
#[derive(Clone, uniffi::Record)]
pub struct Offer {
    /// The full BOLT12 offer string.
    pub string: String,
    /// Offer description, if present.
    pub description: Option<String>,
    /// Offer expiration time (milliseconds since the UNIX epoch).
    pub expires_at_ms: Option<u64>,
    /// Minimum payable amount, in satoshis.
    pub min_amount_sats: Option<u64>,
    /// Self-reported payee name.
    pub payee: Option<String>,
    /// Hex-encoded payee node public key.
    pub payee_pubkey: Option<String>,
}

impl From<OfferRs> for Offer {
    fn from(offer: OfferRs) -> Self {
        Self {
            string: offer.to_string(),
            description: offer.description().map(String::from),
            expires_at_ms: offer.expires_at().map(|t| t.to_millis()),
            min_amount_sats: offer.min_amount().map(|amt| amt.sats_u64()),
            payee: offer.payee().map(String::from),
            payee_pubkey: offer.payee_node_pk().map(|pk| pk.to_string()),
        }
    }
}

/// Response from creating a BOLT 12 offer.
#[derive(Clone, uniffi::Record)]
pub struct CreateOfferResponse {
    /// String-encoded BOLT 12 offer.
    pub offer: String,
}

impl From<SdkCreateOfferResponse> for CreateOfferResponse {
    fn from(resp: SdkCreateOfferResponse) -> Self {
        Self {
            offer: resp.offer.to_string(),
        }
    }
}
