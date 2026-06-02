//! Synchronous (blocking) wrapper around
//! [`LexeWallet`](crate::wallet::LexeWallet).
//!
//! Enabled by the `blocking` feature flag.
//
// All async methods are executed using `block_on`, which wraps the future
// with `async_compat::Compat` so it runs on the shared tokio runtime used
// by UniFFI, then blocks the current thread until the future completes.

use std::{path::PathBuf, time::Duration};

#[cfg(feature = "unstable")]
use crate::unstable;
use crate::{
    config::WalletEnvConfig,
    types::{
        auth::{CredentialsRef, RootSeed, UserPk},
        command::{
            AnalyzeRequest, AnalyzeResponse, CreateInvoiceRequest,
            CreateInvoiceResponse, CreateOfferRequest, CreateOfferResponse,
            GetPaymentRequest, GetPaymentResponse, GetUpdatedPaymentsRequest,
            GetUpdatedPaymentsResponse, ListPaymentsResponse, NodeInfo,
            PayInvoiceRequest, PayInvoiceResponse, PayLnurlRequest,
            PayLnurlResponse, PayOfferRequest, PayOfferResponse, PayRequest,
            PayResponse, PaymentSyncSummary, UpdatePersonalNoteRequest,
            WithdrawLnurlRequest, WithdrawLnurlResponse,
        },
        payment::{Order, Payment, PaymentCreatedIndex, PaymentFilter},
    },
    wallet::LexeWallet,
};

/// Synchronous wallet handle. Provides the same API as [`LexeWallet`] but
/// with blocking methods instead of async.
pub struct BlockingLexeWallet {
    inner: LexeWallet,
}

impl BlockingLexeWallet {
    // --- Constructors --- //

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

    /// Create a [`BlockingLexeWallet`] without any persistence.
    /// It is recommended to use [`fresh`] or [`load`] instead, to initialize
    /// with persistence.
    ///
    /// Node operations (invoices, payments, node info) work normally.
    /// Local payment cache operations ([`sync_payments`], [`list_payments`],
    /// [`clear_payments`]) are not available and will return an error.
    ///
    /// [`fresh`]: BlockingLexeWallet::fresh
    /// [`load`]: BlockingLexeWallet::load
    /// [`sync_payments`]: BlockingLexeWallet::sync_payments
    /// [`list_payments`]: BlockingLexeWallet::list_payments
    /// [`clear_payments`]: BlockingLexeWallet::clear_payments
    pub fn without_db(
        env_config: WalletEnvConfig,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<Self> {
        let inner = LexeWallet::without_db(env_config, credentials)?;
        Ok(Self { inner })
    }

    // --- DB-required methods --- //

    /// Sync payments from the user node to the local payments cache.
    ///
    /// Returns an error if local persistence is disabled for this wallet.
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
    /// Returns an error if local persistence is disabled for this wallet.
    ///
    /// [`sync_payments`]: Self::sync_payments
    pub fn list_payments(
        &self,
        filter: &PaymentFilter,
        order: Option<Order>,
        limit: Option<usize>,
        after: Option<&PaymentCreatedIndex>,
    ) -> anyhow::Result<ListPaymentsResponse> {
        self.inner.list_payments(filter, order, limit, after)
    }

    /// Clear all locally cached payment data for this wallet.
    ///
    /// Clears the local payment cache only. Remote data on the node is not
    /// affected. Call [`sync_payments`](Self::sync_payments) to re-populate.
    ///
    /// Returns an error if local persistence is disabled for this wallet.
    pub fn clear_payments(&self) -> anyhow::Result<()> {
        self.inner.clear_payments()
    }

    /// Wait for a payment to reach a terminal state (completed or failed).
    ///
    /// Polls the node with exponential backoff until the payment finalizes or
    /// the timeout is reached. Defaults to 600 seconds (10 minutes).
    /// Maximum timeout is 86,400 seconds (24 hours).
    pub fn wait_for_payment(
        &self,
        index: PaymentCreatedIndex,
        timeout: Option<Duration>,
    ) -> anyhow::Result<Payment> {
        block_on(self.inner.wait_for_payment(index, timeout))
    }

    /// Get a reference to the
    /// [`WalletDb`](crate::unstable::wallet_db::WalletDb).
    ///
    /// Returns [`None`] if local persistence is disabled for this wallet.
    #[cfg(feature = "unstable")]
    pub fn db(
        &self,
    ) -> Option<&unstable::wallet_db::WalletDb<unstable::ffs::DiskFs>> {
        self.inner.db()
    }

    /// Get a reference to the payments database.
    /// This is the primary data source for constructing a payments
    /// list UI.
    ///
    /// Returns [`None`] if local persistence is disabled for this wallet.
    #[cfg(feature = "unstable")]
    pub fn payments_db(
        &self,
    ) -> Option<&unstable::payments_db::PaymentsDb<unstable::ffs::DiskFs>> {
        self.inner.payments_db()
    }

    // --- Shared methods --- //

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

    /// [`signup`](Self::signup) but with extra parameters generally only used
    /// by the Lexe App.
    #[cfg(feature = "unstable")]
    pub fn signup_custom(
        &self,
        root_seed: &RootSeed,
        partner_pk: Option<UserPk>,
        allow_gvfs_access: bool,
        backup_password: Option<&str>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.signup_custom(
            root_seed,
            partner_pk,
            allow_gvfs_access,
            backup_password,
            google_auth_code,
        ))
    }

    /// Ensures the wallet is provisioned to all recent trusted releases.
    /// This should be called every time the wallet is loaded, to ensure the
    /// node is running the most up-to-date enclave software.
    ///
    /// This fetches the current enclaves from the gateway, computes which
    /// releases need to be provisioned, and provisions them.
    pub fn provision(
        &self,
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.provision(credentials))
    }

    /// [`provision`](Self::provision) but with extra parameters generally only
    /// used by the Lexe App.
    #[cfg(feature = "unstable")]
    pub fn provision_custom(
        &self,
        credentials: CredentialsRef<'_>,
        allow_gvfs_access: bool,
        encrypted_seed: Option<Vec<u8>>,
        google_auth_code: Option<String>,
    ) -> anyhow::Result<()> {
        block_on(self.inner.provision_custom(
            credentials,
            allow_gvfs_access,
            encrypted_seed,
            google_auth_code,
        ))
    }

    /// Get a reference to the
    /// [`GatewayClient`](lexe_node_client::client::GatewayClient).
    #[cfg(feature = "unstable")]
    pub fn gateway_client(&self) -> &lexe_node_client::client::GatewayClient {
        self.inner.gateway_client()
    }

    /// Get a reference to the
    /// [`NodeClient`](lexe_node_client::client::NodeClient).
    #[cfg(feature = "unstable")]
    pub fn node_client(&self) -> &lexe_node_client::client::NodeClient {
        self.inner.node_client()
    }

    /// Get a reference to the
    /// [`Bip353Client`](lexe_payment_uri::bip353::Bip353Client).
    #[cfg(feature = "unstable")]
    pub fn bip353_client(&self) -> &lexe_payment_uri::bip353::Bip353Client {
        self.inner.bip353_client()
    }

    /// Get a reference to the
    /// [`LnurlClient`](lexe_payment_uri::lnurl::LnurlClient).
    #[cfg(feature = "unstable")]
    pub fn lnurl_client(&self) -> &lexe_payment_uri::lnurl::LnurlClient {
        self.inner.lnurl_client()
    }

    // --- Command API --- //

    /// Get information about this Lexe node, including balance and channels.
    pub fn node_info(&self) -> anyhow::Result<NodeInfo> {
        block_on(self.inner.node_info())
    }

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
    ///
    /// [`PaymentMethod`]: lexe_payment_uri::PaymentMethod
    /// [`PayableDetails`]: crate::types::command::PayableDetails
    // Sync the encodings list with `pay`
    pub fn analyze(
        &self,
        req: AnalyzeRequest,
    ) -> anyhow::Result<AnalyzeResponse> {
        block_on(self.inner.analyze(req))
    }

    /// Pay any string which encodes a Bitcoin or Lightning payment method.
    ///
    /// If there exist multiple encoded payment methods, one best recommended
    /// payment method will be chosen.
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
    ///
    /// [`PaymentMethod`]: lexe_payment_uri::PaymentMethod
    // Sync the encodings list with `analyze`
    pub fn pay(&self, req: PayRequest) -> anyhow::Result<PayResponse> {
        block_on(self.inner.pay(req))
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

    /// Create a BOLT 12 offer to receive Lightning payments.
    ///
    /// Unlike invoices, offers are reusable: multiple payments can be made to
    /// it, including from multiple payers.
    pub fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> anyhow::Result<CreateOfferResponse> {
        block_on(self.inner.create_offer(req))
    }

    /// Pay a BOLT 12 offer over Lightning.
    pub fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> anyhow::Result<PayOfferResponse> {
        block_on(self.inner.pay_offer(req))
    }

    /// Pay an LNURL or Lightning Address via the `payRequest` flow.
    pub fn pay_lnurl(
        &self,
        req: PayLnurlRequest,
    ) -> anyhow::Result<PayLnurlResponse> {
        block_on(self.inner.pay_lnurl(req))
    }

    /// Withdraw an LNURL via the `withdrawRequest` flow.
    pub fn withdraw_lnurl(
        &self,
        req: WithdrawLnurlRequest,
    ) -> anyhow::Result<WithdrawLnurlResponse> {
        block_on(self.inner.withdraw_lnurl(req))
    }

    /// Get information about a payment by its created index.
    pub fn get_payment(
        &self,
        req: GetPaymentRequest,
    ) -> anyhow::Result<GetPaymentResponse> {
        block_on(self.inner.get_payment(req))
    }

    /// Get a batch of payments in ascending `updated_at` order, starting from
    /// a given `updated_at` index.
    ///
    /// Useful for tailing / syncing payment updates as they occur and merging
    /// them into a local payments store.
    pub fn get_updated_payments(
        &self,
        req: GetUpdatedPaymentsRequest,
    ) -> anyhow::Result<GetUpdatedPaymentsResponse> {
        block_on(self.inner.get_updated_payments(req))
    }

    /// Update the personal note on an existing payment.
    /// The note is stored on the user node and is not visible to the
    /// counterparty.
    pub fn update_personal_note(
        &self,
        req: UpdatePersonalNoteRequest,
    ) -> anyhow::Result<()> {
        block_on(self.inner.update_personal_note(req))
    }
}

/// Wraps the future with `async_compat::Compat` so it runs on the shared
/// tokio runtime, then blocks with `futures::executor::block_on`.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let f = async_compat::Compat::new(f);
    futures::executor::block_on(f)
}
