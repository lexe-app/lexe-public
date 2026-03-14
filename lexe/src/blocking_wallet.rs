//! Synchronous (blocking) wrapper around
//! [`LexeWallet`](crate::wallet::LexeWallet).
//!
//! Enabled by the `blocking` feature flag.
//
// All async methods are executed using `block_on`, which wraps the future
// with `async_compat::Compat` so it runs on the shared tokio runtime used
// by UniFFI, then blocks the current thread until the future completes.

use std::{path::PathBuf, time::Duration};

use lexe_api::types::payments::PaymentCreatedIndex;
use lexe_common::api::user::UserPk;

use crate::{
    config::WalletEnvConfig,
    types::{
        auth::{CredentialsRef, RootSeed},
        command::{
            CreateInvoiceRequest, CreateInvoiceResponse, GetPaymentRequest,
            GetPaymentResponse, ListPaymentsResponse, NodeInfo,
            PayInvoiceRequest, PayInvoiceResponse, PaymentSyncSummary,
            SdkUpdatePaymentNoteRequest,
        },
        payment::{Order, Payment, PaymentFilter},
    },
    unstable::{ffs::DiskFs, payments_db::PaymentsDb},
    wallet::{LexeWallet, WithDb, WithoutDb},
};

/// Synchronous wallet handle. Provides the same API as [`LexeWallet`] but
/// with blocking methods instead of async.
pub struct BlockingLexeWallet<Db> {
    inner: LexeWallet<Db>,
}

impl BlockingLexeWallet<WithDb> {
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
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let inner = LexeWallet::fresh(env_config, credentials, lexe_data_dir)?;
        Ok(Self { inner })
    }

    /// Load an existing [`BlockingLexeWallet`] with persistence from
    /// `lexe_data_dir`. Returns [`None`] if no local data exists, in which
    /// case you should use [`fresh`](Self::fresh) to create the wallet.
    ///
    /// If you are authenticating with [`RootSeed`]s and this returns [`None`],
    /// you should call [`signup`](Self::signup) after creating the wallet if
    /// you're not sure whether the user has been signed up with Lexe.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this wallet is for. Users and environments will not interfere
    /// with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn load(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Option<Self>> {
        LexeWallet::load(env_config, credentials, lexe_data_dir)
            .map(|opt| opt.map(|inner| Self { inner }))
    }

    /// Load an existing [`BlockingLexeWallet`] with persistence from
    /// `lexe_data_dir`, or create a fresh one if no local data exists. If you
    /// are authenticating with client credentials, this is generally what you
    /// want to use.
    ///
    /// It is recommended to always pass the same `lexe_data_dir`,
    /// regardless of which environment we're in (dev/staging/prod) and which
    /// user this wallet is for. Users and environments will not interfere
    /// with each other as all data is namespaced internally.
    /// Defaults to `~/.lexe` if not specified.
    pub fn load_or_fresh(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
        lexe_data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let inner =
            LexeWallet::load_or_fresh(env_config, credentials, lexe_data_dir)?;
        Ok(Self { inner })
    }

    /// Get a reference to the payments database.
    /// This is the primary data source for constructing a payments list UI.
    pub fn payments_db(&self) -> &PaymentsDb<DiskFs> {
        self.inner.payments_db()
    }

    /// Sync payments from the user node to the local database.
    /// This fetches updated payments from the node and persists them locally.
    pub fn sync_payments(&self) -> anyhow::Result<PaymentSyncSummary> {
        block_on(self.inner.sync_payments())
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
    /// [`sync_payments`]: Self::sync_payments
    pub fn list_payments(
        &self,
        filter: &PaymentFilter,
        order: Option<Order>,
        limit: Option<usize>,
        after: Option<&PaymentCreatedIndex>,
    ) -> ListPaymentsResponse {
        self.inner.list_payments(filter, order, limit, after)
    }

    /// Clear all local payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call [`sync_payments`](Self::sync_payments) to re-populate.
    pub fn clear_payments(&self) -> anyhow::Result<()> {
        self.inner.clear_payments()
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 10 minutes if not specified.
    /// Maximum timeout is 86,400 seconds (24 hours).
    pub fn wait_for_payment(
        &self,
        index: PaymentCreatedIndex,
        timeout: Option<Duration>,
    ) -> anyhow::Result<Payment> {
        block_on(self.inner.wait_for_payment(index, timeout))
    }
}

impl BlockingLexeWallet<WithoutDb> {
    /// Create a [`BlockingLexeWallet`] without any persistence.
    /// It is recommended to use [`fresh`] or [`load`] instead, to initialize
    /// with persistence.
    ///
    /// [`fresh`]: BlockingLexeWallet::fresh
    /// [`load`]: BlockingLexeWallet::load
    pub fn without_db(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<Self> {
        let inner = LexeWallet::without_db(env_config, credentials)?;
        Ok(Self { inner })
    }
}

impl<D> BlockingLexeWallet<D> {
    /// Get a reference to the user's wallet configuration.
    pub fn user_config(&self) -> &crate::config::WalletUserConfig {
        self.inner.user_config()
    }

    /// Registers this user with Lexe, then provisions the node.
    /// This method must be called after the user's [`BlockingLexeWallet`]
    /// has been created for the first time, otherwise subsequent requests
    /// will fail.
    ///
    /// It is only necessary to call this method once, ever, per user, but
    /// it is also okay to call this method even if the user has already
    /// been signed up; in other words, this method is idempotent.
    ///
    /// After a successful signup, make sure the user's root seed has been
    /// persisted somewhere! Without access to their root seed, your user
    /// will lose their funds forever. If adding Lexe to a broader wallet,
    /// a good strategy is to derive Lexe's [`RootSeed`] from your own
    /// root seed.
    ///
    /// - `partner_pk`: Set to your company's [`UserPk`] to earn a share of this
    ///   wallet's fees.
    pub fn signup(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.signup(root_seed, partner_pk))
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    /// This should be called every time the wallet is loaded, to ensure the
    /// user is running the most up-to-date enclave software.
    ///
    /// This fetches the current enclaves from the gateway, computes which
    /// releases need to be provisioned, and provisions them.
    pub fn provision(
        &self,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.provision(credentials))
    }

    /// Get information about this Lexe node, including balance and channels.
    pub fn node_info(&self) -> anyhow::Result<NodeInfo> {
        block_on(self.inner.node_info())
    }

    /// Create a BOLT 11 invoice to receive a Lightning payment.
    pub fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> anyhow::Result<CreateInvoiceResponse> {
        block_on(self.inner.create_invoice(req))
    }

    /// Pay a BOLT 11 invoice over Lightning.
    pub fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> anyhow::Result<PayInvoiceResponse> {
        block_on(self.inner.pay_invoice(req))
    }

    /// Get information about a payment by its created index.
    pub fn get_payment(
        &self,
        req: GetPaymentRequest,
    ) -> anyhow::Result<GetPaymentResponse> {
        block_on(self.inner.get_payment(req))
    }

    /// Update the personal note on an existing payment.
    /// The note is stored on the user node and is not visible to the
    /// counterparty.
    pub fn update_payment_note(
        &self,
        req: SdkUpdatePaymentNoteRequest,
    ) -> anyhow::Result<()> {
        block_on(self.inner.update_payment_note(req))
    }
}

/// Wraps the future with `async_compat::Compat` so it runs on the shared
/// tokio runtime, then blocks with `futures::executor::block_on`.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let f = async_compat::Compat::new(f);
    futures::executor::block_on(f)
}
