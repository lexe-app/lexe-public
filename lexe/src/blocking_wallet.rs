//! Synchronous (blocking) wrapper around
//! [`LexeWallet`](crate::wallet::LexeWallet).
//!
//! Enabled by the `blocking` feature flag. All async methods are executed
//! using `block_on`, which wraps the future with [`async_compat::Compat`]
//! so it runs on the shared tokio runtime used by UniFFI, then blocks the
//! current thread until the future completes.

use std::path::PathBuf;

use common::{api::user::UserPk, rng::Crng, root_seed::RootSeed};
use lexe_api::models::command::UpdatePaymentNote;
use node_client::credentials::CredentialsRef;
use sdk_core::models::{
    SdkCreateInvoiceRequest, SdkCreateInvoiceResponse, SdkGetPaymentRequest,
    SdkGetPaymentResponse, SdkNodeInfo, SdkPayInvoiceRequest,
    SdkPayInvoiceResponse,
};

use crate::{
    config::WalletEnvConfig,
    payments_db::{PaymentSyncSummary, PaymentsDb},
    unstable::ffs::DiskFs,
    wallet::{LexeWallet, WithDb},
};

/// Block the current thread on an async future.
///
/// Wraps the future with [`async_compat::Compat`] so it can use the shared
/// tokio runtime (the same one UniFFI uses for `async_runtime = "tokio"`),
/// then blocks with [`futures::executor::block_on`].
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let f = async_compat::Compat::new(f);
    futures::executor::block_on(f)
}

/// Synchronous wallet handle wrapping a [`LexeWallet`].
///
/// Every async method on `LexeWallet` has a blocking counterpart here that
/// calls `block_on` internally.
pub struct BlockingLexeWallet {
    inner: LexeWallet<WithDb>,
}

impl BlockingLexeWallet {
    /// Create a fresh [`BlockingLexeWallet`], deleting any existing database
    /// state for this user. Data for other users and environments is not
    /// affected.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this wallet is for. Users and environments will not interfere
    /// with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn fresh(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let inner =
            LexeWallet::fresh(rng, env_config, credentials, lexe_data_dir)?;
        Ok(Self { inner })
    }

    /// Load an existing [`BlockingLexeWallet`] with persistence from
    /// `lexe_data_dir`. Returns [`None`] if no local data exists, in which
    /// case you should use [`fresh`](Self::fresh) to create the wallet.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this wallet is for. Users and environments will not interfere
    /// with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn load(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Option<Self>> {
        LexeWallet::load(rng, env_config, credentials, lexe_data_dir)
            .map(|opt| opt.map(|inner| Self { inner }))
    }

    /// Load an existing [`BlockingLexeWallet`] with persistence from
    /// `lexe_data_dir`, or create a fresh one if no local data exists.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this wallet is for. Users and environments will not interfere
    /// with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn load_or_fresh(
        rng: &mut impl Crng,
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let inner = LexeWallet::load_or_fresh(
            rng,
            env_config,
            credentials,
            lexe_data_dir,
        )?;
        Ok(Self { inner })
    }

    /// Get a reference to the [`PaymentsDb`].
    /// This is the primary data source for constructing a payments list UI.
    pub fn payments_db(&self) -> &PaymentsDb<DiskFs> {
        self.inner.payments_db()
    }

    /// Sync payments from the user node to the local database.
    /// This fetches updated payments from the node and persists them locally.
    ///
    /// Only one sync can run at a time.
    /// Errors if another sync is already in progress.
    pub fn sync_payments(&self) -> anyhow::Result<PaymentSyncSummary> {
        block_on(self.inner.sync_payments())
    }

    /// Get a reference to the user's wallet configuration.
    pub fn user_config(&self) -> &crate::config::WalletUserConfig {
        self.inner.user_config()
    }

    /// Registers this user with the Lexe backend, then provisions the node.
    ///
    /// It is only necessary to call this function once, ever, per user, but
    /// it is also okay to call it again; this function is idempotent.
    ///
    /// After a successful signup, make sure the user's root seed has been
    /// persisted somewhere! Without access to their root seed, your user
    /// will lose their funds forever.
    ///
    /// - `partner_pk`: Set to your company's [`UserPk`] to earn a share of this
    ///   wallet's fees.
    pub fn signup(
        &self,
        rng: &mut impl Crng,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.signup(rng, root_seed, partner_pk))
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    ///
    /// This should be called every time the wallet is loaded, to ensure the
    /// user is running the most up-to-date enclave software. Fetches current
    /// enclaves from the gateway and provisions any that need updating.
    pub fn provision(
        &self,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.provision(credentials))
    }

    /// Get information about this Lexe node.
    pub fn node_info(&self) -> anyhow::Result<SdkNodeInfo> {
        block_on(self.inner.node_info())
    }

    /// Create a BOLT 11 invoice.
    pub fn create_invoice(
        &self,
        req: SdkCreateInvoiceRequest,
    ) -> anyhow::Result<SdkCreateInvoiceResponse> {
        block_on(self.inner.create_invoice(req))
    }

    /// Pay a BOLT 11 invoice.
    pub fn pay_invoice(
        &self,
        req: SdkPayInvoiceRequest,
    ) -> anyhow::Result<SdkPayInvoiceResponse> {
        block_on(self.inner.pay_invoice(req))
    }

    /// Get information about a payment by its index.
    pub fn get_payment(
        &self,
        req: SdkGetPaymentRequest,
    ) -> anyhow::Result<SdkGetPaymentResponse> {
        block_on(self.inner.get_payment(req))
    }

    /// Update the note on an existing payment.
    pub fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        block_on(self.inner.update_payment_note(req))
    }
}
