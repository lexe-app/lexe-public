//! Lexe SDK foreign language bindings.
//!
//! This crate is the [UniFFI] base for generating Lexe SDK bindings in
//! languages like Python, Javascript, Swift, and Kotlin.
//!
//! For Rust projects, use the [`lexe`] crate directly.
//!
//! [UniFFI]: https://mozilla.github.io/uniffi-rs/

use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use common::{
    api::user::UserPk,
    env::DeployEnv as DeployEnvRs,
    ln::{
        amount::Amount as AmountRs, network::LxNetwork as LxNetworkRs,
        priority::ConfirmationPriority as ConfirmationPriorityRs,
    },
    rng::SysRng,
    root_seed::RootSeed as RootSeedRs,
};
use lexe::{
    config::WalletEnvConfig as WalletEnvConfigRs,
    types::{
        SdkCreateInvoiceRequest as SdkCreateInvoiceRequestRs,
        SdkCreateInvoiceResponse as SdkCreateInvoiceResponseRs,
        SdkGetPaymentRequest as SdkGetPaymentRequestRs,
        SdkNodeInfo as SdkNodeInfoRs,
        SdkPayInvoiceRequest as SdkPayInvoiceRequestRs,
        SdkPayInvoiceResponse as SdkPayInvoiceResponseRs,
        SdkPayment as SdkPaymentRs,
    },
    wallet::LexeWallet as LexeWalletRs,
};
use lexe_api_core::{
    error::GatewayApiError as GatewayApiErrorRs,
    models::command::UpdatePaymentNote as UpdatePaymentNoteRs,
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
use lexe_std::backoff;
use node_client::credentials::{
    ClientCredentials as ClientCredentialsRs, CredentialsRef,
};
use secrecy::{ExposeSecret, Secret};

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

/// Returns the path to the seedphrase file for the given environment.
///
/// - Mainnet: `<lexe_data_dir>/seedphrase.txt`
/// - Other environments: `<lexe_data_dir>/seedphrase.<env>.txt`
#[uniffi::export]
pub fn seedphrase_path(
    env_config: Arc<WalletEnvConfig>,
    lexe_data_dir: String,
) -> String {
    env_config
        .to_rs()
        .seedphrase_path(lexe_data_dir.as_ref())
        .to_string_lossy()
        .into_owned()
}

/// Reads a root seed from `~/.lexe/seedphrase[.env].txt`.
///
/// Returns `None` if the file doesn't exist.
#[uniffi::export]
pub fn read_seed(
    env_config: Arc<WalletEnvConfig>,
) -> FfiResult<Option<RootSeed>> {
    env_config
        .to_rs()
        .read_seed()
        .map(|opt| {
            opt.map(|seed| RootSeed {
                seed_bytes: seed.expose_secret().to_vec(),
            })
        })
        .map_err(Into::into)
}

/// Reads a root seed from a seedphrase file containing a BIP39 mnemonic.
///
/// Returns `None` if the file doesn't exist.
#[uniffi::export]
pub fn read_seed_from_path(path: String) -> FfiResult<Option<RootSeed>> {
    RootSeedRs::read_from_path(path.as_ref())
        .map(|opt| {
            opt.map(|seed| RootSeed {
                seed_bytes: seed.expose_secret().to_vec(),
            })
        })
        .map_err(Into::into)
}

/// Writes a root seed's mnemonic to `~/.lexe/seedphrase[.env].txt`.
///
/// Creates parent directories if needed. Returns an error if the file
/// already exists.
#[uniffi::export]
pub fn write_seed(
    root_seed: RootSeed,
    env_config: Arc<WalletEnvConfig>,
) -> FfiResult<()> {
    let root_seed_rs = root_seed.to_rs()?;
    env_config
        .to_rs()
        .write_seed(&root_seed_rs)
        .map_err(Into::into)
}

/// Writes a root seed's mnemonic to a seedphrase file.
///
/// Creates parent directories if needed. Returns an error if the file
/// already exists.
#[uniffi::export]
pub fn write_seed_to_path(root_seed: RootSeed, path: String) -> FfiResult<()> {
    let root_seed_rs = root_seed.to_rs()?;
    root_seed_rs
        .write_to_path(path.as_ref())
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
#[derive(Clone, uniffi::Record)]
pub struct RootSeed {
    /// 32-byte root seed.
    pub seed_bytes: Vec<u8>,
}

impl RootSeed {
    fn to_rs(&self) -> FfiResult<RootSeedRs> {
        let seed_bytes: [u8; 32] = self
            .seed_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid seed length"))?;
        Ok(RootSeedRs::new(Secret::new(seed_bytes)))
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

impl From<SdkNodeInfoRs> for NodeInfo {
    fn from(info: SdkNodeInfoRs) -> Self {
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

// ================== //
// --- LexeWallet --- //
// ================== //

/// The main wallet handle for interacting with Lexe.
#[derive(uniffi::Object)]
pub struct LexeWallet {
    inner: Arc<LexeWalletRs<lexe::wallet::WithDb>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl LexeWallet {
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
        root_seed: RootSeed,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let mut rng = SysRng::new();
        let root_seed_rs = root_seed.to_rs()?;
        let credentials = CredentialsRef::from(&root_seed_rs);
        let env_config_rs = env_config.to_rs();

        let inner = LexeWalletRs::fresh(
            &mut rng,
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: Arc::new(inner),
        }))
    }

    /// Load an existing wallet, or create a fresh one if no local data exists.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`, regardless of
    /// environment (dev/staging/prod) or user. Data is namespaced internally,
    /// so users and environments do not interfere with each other.
    /// Defaults to `~/.lexe` if not specified.
    #[uniffi::constructor(default(lexe_data_dir = None))]
    pub fn load_or_fresh(
        env_config: Arc<WalletEnvConfig>,
        root_seed: RootSeed,
        lexe_data_dir: Option<String>,
    ) -> FfiResult<Arc<Self>> {
        let mut rng = SysRng::new();
        let root_seed_rs = root_seed.to_rs()?;
        let credentials = CredentialsRef::from(&root_seed_rs);
        let env_config_rs = env_config.to_rs();

        let inner = LexeWalletRs::load_or_fresh(
            &mut rng,
            env_config_rs,
            credentials,
            lexe_data_dir.map(PathBuf::from),
        )?;

        Ok(Arc::new(Self {
            inner: Arc::new(inner),
        }))
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
        root_seed: RootSeed,
        partner_pk: Option<String>,
    ) -> FfiResult<()> {
        let mut rng = SysRng::new();
        let root_seed_rs = root_seed.to_rs()?;
        let partner = partner_pk
            .as_deref()
            .map(|s| {
                s.parse::<UserPk>().map_err(|e| {
                    anyhow::anyhow!("Invalid partner user_pk: {e}")
                })
            })
            .transpose()?;

        self.inner.signup(&mut rng, &root_seed_rs, partner).await?;
        Ok(())
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// Call this every time the wallet is loaded to ensure the user is running
    /// the most up-to-date enclave software. Fetches current enclaves from the
    /// gateway and provisions any that need updating.
    pub async fn provision(&self, root_seed: RootSeed) -> FfiResult<()> {
        let root_seed_rs = root_seed.to_rs()?;
        let credentials = CredentialsRef::from(&root_seed_rs);
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
    pub async fn create_invoice(
        &self,
        expiration_secs: u32,
        amount_sats: Option<u64>,
        description: Option<String>,
    ) -> FfiResult<CreateInvoiceResponse> {
        let amount = amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid amount: {e}"))?;

        let req = SdkCreateInvoiceRequestRs {
            expiration_secs,
            amount,
            description,
        };
        let resp = self.inner.create_invoice(req).await?;
        Ok(resp.into())
    }

    /// Pay a BOLT11 invoice.
    /// `fallback_amount_sats` is required if the invoice is amountless.
    /// `note` is a private note that the receiver does not see.
    pub async fn pay_invoice(
        &self,
        invoice: String,
        fallback_amount_sats: Option<u64>,
        note: Option<String>,
    ) -> FfiResult<PayInvoiceResponse> {
        let invoice: LxInvoiceRs = invoice
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid invoice: {e}"))?;
        let fallback_amount = fallback_amount_sats
            .map(AmountRs::try_from_sats_u64)
            .transpose()
            .map_err(|e| anyhow::anyhow!("Invalid fallback amount: {e}"))?;

        let req = SdkPayInvoiceRequestRs {
            invoice,
            fallback_amount,
            note,
        };
        let resp = self.inner.pay_invoice(req).await?;
        Ok(resp.into())
    }

    /// Get a payment by its `payment_index` string.
    pub async fn get_payment(
        &self,
        payment_index: String,
    ) -> FfiResult<Option<Payment>> {
        let index = parse_payment_index(&payment_index)?;
        let req = SdkGetPaymentRequestRs { index };
        let resp = self.inner.get_payment(req).await?;
        Ok(resp.payment.map(Into::into))
    }

    /// Update a payment's note.
    /// Call `sync_payments` first so the payment exists locally.
    pub async fn update_payment_note(
        &self,
        payment_index: String,
        note: Option<String>,
    ) -> FfiResult<()> {
        let index = parse_payment_index(&payment_index)?;
        let req = UpdatePaymentNoteRs { index, note };
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

    /// List payments from local storage based on filters.
    /// `offset` and `limit` are for pagination over local storage.
    pub fn list_payments(
        &self,
        filter: PaymentFilter,
        offset: u32,
        limit: u32,
    ) -> ListPaymentsResponse {
        let db = self.inner.payments_db();
        let offset = offset as usize;
        let limit = limit as usize;

        let (total_count, payments): (usize, Vec<Payment>) = match filter {
            PaymentFilter::All => (
                db.num_payments(),
                (offset..)
                    .take(limit)
                    .filter_map(|idx| db.get_payment_by_scroll_idx(idx))
                    .map(|p| Payment::from(SdkPaymentRs::from(p)))
                    .collect(),
            ),
            PaymentFilter::Pending => (
                db.num_pending(),
                (offset..)
                    .take(limit)
                    .filter_map(|idx| db.get_pending_payment_by_scroll_idx(idx))
                    .map(|p| Payment::from(SdkPaymentRs::from(p)))
                    .collect(),
            ),
            PaymentFilter::Finalized => (
                db.num_finalized(),
                (offset..)
                    .take(limit)
                    .filter_map(|idx| {
                        db.get_finalized_payment_by_scroll_idx(idx)
                    })
                    .map(|p| Payment::from(SdkPaymentRs::from(p)))
                    .collect(),
            ),
        };

        ListPaymentsResponse {
            payments,
            total_count: total_count as u64,
        }
    }

    /// Get the latest payment sync index (watermark).
    ///
    /// Returns the `updated_at` index of the most recently synced payment,
    /// or `None` if no payments have been synced yet.
    pub fn latest_payment_sync_index(&self) -> Option<String> {
        self.inner
            .payments_db()
            .latest_updated_index()
            .map(|idx| idx.to_string())
    }

    /// Delete all local payment data for this wallet.
    ///
    /// This clears the local payment cache. Remote data on the node is not
    /// affected. Call `sync_payments` to re-populate from the node.
    pub fn delete_local_payments(&self) -> FfiResult<()> {
        self.inner
            .payments_db()
            .delete()
            .context("Failed to delete local payments")?;
        Ok(())
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    /// Uses exponential backoff polling under the hood.
    /// Recommended timeout is 120 seconds.
    /// Maximum timeout is 10_800 seconds (3 hours).
    // TODO(max): We should either delete this or move into `lexe`
    pub async fn wait_for_payment_completion(
        &self,
        payment_index: String,
        timeout_secs: u32,
    ) -> FfiResult<Payment> {
        const MAX_TIMEOUT_SECS: u32 = 3 * 60 * 60;
        if timeout_secs > MAX_TIMEOUT_SECS {
            return Err(anyhow::anyhow!(
                "timeout_secs exceeds max of {MAX_TIMEOUT_SECS}s (3 hours): {timeout_secs}s"
            )
            .into());
        }

        let timeout = Duration::from_secs(timeout_secs.into());
        let start = tokio::time::Instant::now();
        let index = parse_payment_index(&payment_index)?;
        let mut backoff = backoff::iter_with_initial_wait_ms(1_000);

        loop {
            self.inner.sync_payments().await?;

            if let Some(payment) = self
                .inner
                .payments_db()
                .get_payment_by_created_index(&index)
            {
                let payment = SdkPaymentRs::from(payment);
                match payment.status {
                    PaymentStatusRs::Completed | PaymentStatusRs::Failed => {
                        return Ok(Payment::from(payment));
                    }
                    PaymentStatusRs::Pending => {
                        // Continue polling
                    }
                }
            }

            if start.elapsed() >= timeout {
                return Err(anyhow::anyhow!(
                    "Payment did not complete within {timeout_secs}s timeout"
                )
                .into());
            }

            tokio::time::sleep(backoff.next_delay()).await;
        }
    }
}

/// Try to load an existing wallet; return [`None`] if no local data exists.
///
/// If this returns [`None`], call [`LexeWallet::fresh`] to create local state.
///
/// It is recommended to always pass the same `lexe_data_dir`, regardless of
/// environment (dev/staging/prod) or user. Data is namespaced internally, so
/// users and environments do not interfere with each other.
/// Defaults to `~/.lexe` if not specified.
// This is a free function because uniffi constructors cannot return
// `Option<Arc<Self>>`, and uniffi doesn't support static methods that
// aren't constructors.
#[uniffi::export(default(lexe_data_dir = None))]
pub fn try_load_wallet(
    env_config: Arc<WalletEnvConfig>,
    root_seed: RootSeed,
    lexe_data_dir: Option<String>,
) -> FfiResult<Option<Arc<LexeWallet>>> {
    let mut rng = SysRng::new();
    let root_seed_rs = root_seed.to_rs()?;
    let credentials = CredentialsRef::from(&root_seed_rs);
    let env_config_rs = env_config.to_rs();

    let maybe_inner = LexeWalletRs::load(
        &mut rng,
        env_config_rs,
        credentials,
        lexe_data_dir.map(PathBuf::from),
    )?;

    Ok(maybe_inner.map(|inner| {
        Arc::new(LexeWallet {
            inner: Arc::new(inner),
        })
    }))
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
// TODO(max): Consider adding NotJunk variants (PendingNotJunk,
// FinalizedNotJunk) to match PaymentsDb methods: num_pending_not_junk,
// num_finalized_not_junk, get_pending_not_junk_payment_by_scroll_idx, etc.
#[derive(Clone, uniffi::Enum)]
pub enum PaymentFilter {
    /// Include all payments.
    All,
    /// Include only pending payments.
    Pending,
    /// Include only finalized payments (completed or failed).
    Finalized,
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

/// Parse a payment_index string into a PaymentCreatedIndexRs.
fn parse_payment_index(
    payment_index: &str,
) -> FfiResult<PaymentCreatedIndexRs> {
    payment_index
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid payment_index: {e}").into())
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
    /// Full payment index (format: `<created_at_ms>-<payment_id>`).
    /// Used for database lookups and uniquely identifies a payment.
    pub payment_index: String,
    /// Payment identifier without the timestamp.
    pub payment_id: String,
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

impl From<SdkPaymentRs> for Payment {
    fn from(payment: SdkPaymentRs) -> Self {
        let index = PaymentCreatedIndexRs {
            created_at: payment.created_at,
            id: payment.id,
        };
        Self {
            payment_index: index.to_string(),
            payment_id: payment.id.to_string(),
            created_at_ms: payment.created_at.to_millis(),
            updated_at_ms: payment.updated_at.to_millis(),
            rail: payment.rail.into(),
            kind: payment.kind.into(),
            direction: payment.direction.into(),
            status: payment.status.into(),
            status_msg: payment.status_msg,
            amount_sats: payment.amount.map(|a| a.sats_u64()),
            fees_sats: payment.fees.sats_u64(),
            note: payment.note,
            invoice: payment.invoice.as_ref().map(Invoice::from),
            txid: payment.txid.map(|t| t.to_string()),
            address: payment
                .address
                .as_ref()
                .map(|a| a.assume_checked_ref().to_string()),
            expires_at_ms: payment.expires_at.map(|t| t.to_millis()),
            finalized_at_ms: payment.finalized_at.map(|t| t.to_millis()),
            payer_name: payment.payer_name,
            payer_note: payment.payer_note,
            priority: payment.priority.map(Into::into),
        }
    }
}

/// Summary of a payment sync operation.
//
// Skipped: Not exposing `any_changes()` helper from PaymentSyncSummary
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
    /// Payments in the requested window.
    pub payments: Vec<Payment>,
    /// Total number of payments in local storage for this filter.
    pub total_count: u64,
}

// ================ //
// --- Invoices --- //
// ================ //

/// Response from creating an invoice.
#[derive(Clone, uniffi::Record)]
pub struct CreateInvoiceResponse {
    /// Payment created index for this invoice.
    pub payment_index: String,
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

impl From<SdkCreateInvoiceResponseRs> for CreateInvoiceResponse {
    fn from(resp: SdkCreateInvoiceResponseRs) -> Self {
        Self {
            payment_index: resp.index.to_string(),
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
    /// Payment created index for this payment.
    pub payment_index: String,
    /// When we tried to pay this invoice (milliseconds since the UNIX
    /// epoch).
    pub created_at_ms: u64,
}

impl From<SdkPayInvoiceResponseRs> for PayInvoiceResponse {
    fn from(resp: SdkPayInvoiceResponseRs) -> Self {
        Self {
            payment_index: resp.index.to_string(),
            created_at_ms: resp.created_at.to_millis(),
        }
    }
}
