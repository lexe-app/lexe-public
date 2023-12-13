//! # Rust/Dart FFI bindings
//!
//! ## TL;DR: REGENERATE THE BINDINGS
//!
//! If you update this file, re-run:
//!
//! ```bash
//! $ just app-rs-codegen
//! ```
//!
//! ## Overview
//!
//! This file contains all types and functions exposed to Dart. All `pub`
//! functions, structs, and enums in this file also have corresponding
//! representations in the generated Dart code.
//!
//! The generated Dart interface lives in
//! `../../app/lib/bindings_generated_api.dart` (definitions) and
//! `../../app/lib/bindings_generated.dart` (impls).
//!
//! The low-level generated Rust C-ABI interface is in
//! [`crate::bindings_generated`].
//!
//! These FFI bindings are generated using the `app-rs-codegen` crate. Be sure
//! to re-run the `app-rs-codegen` whenever this file changes.
//!
//! ## Understanding the codegen
//!
//! * Both platforms have different representations for most common types like
//!   usize and String.
//! * Basic types are copied to the native platform representation when crossing
//!   the FFI boundary.
//! * For example strings are necessarily copied, as Rust uses utf-8 encoded
//!   strings while Dart uses utf-16 encoded strings.
//! * There are a few special cases where we can avoid copying, like returning a
//!   `ZeroCopyBuffer<Vec<u8>>` from Rust, which becomes a `Uint8List` on the
//!   Dart side without a copy, since Rust can prove there are no borrows to the
//!   owned buffer when it's transferred.
//! * Normal looking pub functions, like `pub fn x() -> u32 { 123 }` look like
//!   async fn's on the Dart side and are run on a separate small threadpool on
//!   the Rust side to avoid blocking the main Flutter UI isolate.
//! * Functions that return `SyncReturn<_>` do block the calling Dart isolate
//!   and are run in-place on that isolate.
//! * `SyncReturn` has ~10x less overhead. Think a few 50-100 ns vs a few µs
//!   overhead per call.
//! * We have to be careful about blocking the main UI isolate, since we only
//!   have 16 ms frame budget to compute and render the UI to maintain a smooth
//!   60 fps. Any ffi that runs for longer than maybe 1 ms should definitely run
//!   as a separate task on the threadpool. Just reading a value out of some
//!   in-memory state is probably cheaper overall to use `SyncReturn`.

use std::{convert::TryFrom, future::Future, str::FromStr, sync::LazyLock};

use anyhow::{anyhow, Context};
pub use common::ln::payments::BasicPayment;
use common::{
    api::{
        command::{
            EstimateFeeSendOnchainRequest as EstimateFeeSendOnchainRequestRs,
            EstimateFeeSendOnchainResponse as EstimateFeeSendOnchainResponseRs,
            FeeEstimate as FeeEstimateRs, NodeInfo as NodeInfoRs,
            SendOnchainRequest as SendOnchainRequestRs,
        },
        def::{AppGatewayApi, AppNodeRunApi},
        fiat_rates::FiatRates as FiatRatesRs,
    },
    ln::{
        amount::Amount,
        payments::{
            ClientPaymentId as ClientPaymentIdRs,
            PaymentDirection as PaymentDirectionRs,
            PaymentKind as PaymentKindRs, PaymentStatus as PaymentStatusRs,
        },
        ConfirmationPriority as ConfirmationPriorityRs,
    },
    rng::SysRng,
};
use flutter_rust_bridge::{
    frb,
    handler::{ReportDartErrorHandler, ThreadPoolExecutor},
    RustOpaque, StreamSink, SyncReturn,
};

pub use crate::app::App;
use crate::{
    dart_task_handler::LxHandler, form, logger, secret_store::SecretStore,
};

// TODO(phlip9): land real async support in flutter_rust_bridge
// As a temporary unblock to support async fn's, we'll just `RUNTIME.block_on`
// with a global tokio runtime in each worker thread.
//
// flutter_rust_bridge defaults to 4 worker threads in its threadpool.
// Consequently, at most 4 top-level tasks will run concurrently before the
// 5'th task needs to wait for an frb worker thread to open up.
//
// Ex:
//
// ```dart
// unawaited(app.node_info());
// unawaited(app.node_info());
// unawaited(app.node_info());
// unawaited(app.node_info());
// unawaited(app.node_info()); // << this request will only start once one of
//                             //    the previous four requests finishes.
// ```
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        // We only need one background worker. `RUNTIME.block_on` will run the
        // task on the calling worker thread, while `tokio::spawn` will spawn
        // the task on this one background worker thread.
        .worker_threads(1)
        .build()
        .expect("Failed to build tokio Runtime")
});

pub(crate) static FLUTTER_RUST_BRIDGE_HANDLER: LazyLock<LxHandler> =
    LazyLock::new(|| {
        // TODO(phlip9): Get backtraces symbolizing correctly on mobile. I'm at
        // a bit of a loss as to why I can't get this working...

        // std::env::set_var("RUST_BACKTRACE", "1");

        // TODO(phlip9): If we want backtraces from panics, we'll need to set a
        // custom panic handler here that formats the backtrace into the panic
        // message string instead of printing it out to stderr (since mobile
        // doesn't show stdout/stderr...)

        let error_handler = ReportDartErrorHandler;
        LxHandler::new(ThreadPoolExecutor::new(error_handler), error_handler)
    });

#[frb(dart_metadata=("freezed"))]
pub struct NodeInfo {
    pub node_pk: String,
    pub local_balance_sats: u64,
}

impl From<NodeInfoRs> for NodeInfo {
    fn from(info: NodeInfoRs) -> Self {
        Self {
            node_pk: info.node_pk.to_string(),
            local_balance_sats: info.local_balance.sats_u64(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct FiatRates {
    pub timestamp_ms: i64,
    // Sadly, the bridge doesn't currently support maps or tuples so... we'll
    // settle for a list...
    pub rates: Vec<FiatRate>,
}

#[frb(dart_metadata=("freezed"))]
pub struct FiatRate {
    pub fiat: String,
    pub rate: f64,
}

impl From<FiatRatesRs> for FiatRates {
    fn from(value: FiatRatesRs) -> Self {
        Self {
            timestamp_ms: value.timestamp_ms.as_i64(),
            rates: value
                .rates
                .into_iter()
                .map(|(fiat, rate)| FiatRate {
                    fiat: fiat.0,
                    rate: rate.0,
                })
                .collect(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeployEnv {
    Prod,
    Staging,
    Dev,
}

impl DeployEnv {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Prod => "prod",
            Self::Staging => "staging",
            Self::Dev => "dev",
        }
    }
}

impl FromStr for DeployEnv {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "prod" => Ok(Self::Prod),
            "staging" => Ok(Self::Staging),
            "dev" => Ok(Self::Dev),
            _ => Err(anyhow!("unrecognized DEPLOY_ENVIRONMENT: '{s}'")),
        }
    }
}

impl From<common::env::DeployEnv> for DeployEnv {
    fn from(env: common::env::DeployEnv) -> Self {
        use common::env::DeployEnv::*;
        match env {
            Dev => Self::Dev,
            Staging => Self::Staging,
            Prod => Self::Prod,
        }
    }
}

impl From<DeployEnv> for common::env::DeployEnv {
    fn from(env: DeployEnv) -> Self {
        use DeployEnv::*;
        match env {
            Dev => Self::Dev,
            Staging => Self::Staging,
            Prod => Self::Prod,
        }
    }
}

// TODO(phlip9): ffs dart doesn't allow methods on plain enums... if FRB always
// gen'd "enhanced" enums, then I could use an associated fn.
//
// "enhanced" enums: <https://dart.dev/language/enums#declaring-enhanced-enums>
pub fn deploy_env_from_str(s: String) -> anyhow::Result<SyncReturn<DeployEnv>> {
    DeployEnv::from_str(&s).map(SyncReturn)
}

#[derive(Clone, Copy, Debug)]
pub enum Network {
    Mainnet,
    Testnet,
    Regtest,
}

impl From<Network> for common::cli::Network {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => common::cli::Network::MAINNET,
            Network::Testnet => common::cli::Network::TESTNET,
            Network::Regtest => common::cli::Network::REGTEST,
        }
    }
}

impl TryFrom<common::cli::Network> for Network {
    type Error = anyhow::Error;

    fn try_from(network: common::cli::Network) -> anyhow::Result<Self> {
        match network {
            common::cli::Network::MAINNET => Ok(Self::Mainnet),
            common::cli::Network::TESTNET => Ok(Self::Testnet),
            common::cli::Network::REGTEST => Ok(Self::Regtest),
            _ => Err(anyhow!("unsupported NETWORK: '{network}'")),
        }
    }
}

pub fn network_from_str(s: String) -> anyhow::Result<SyncReturn<Network>> {
    common::cli::Network::from_str(&s)
        .and_then(Network::try_from)
        .map(SyncReturn)
}

/// Dart-serializable configuration we get from the flutter side.
#[frb(dart_metadata=("freezed"))]
pub struct Config {
    pub deploy_env: DeployEnv,
    pub network: Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub base_app_data_dir: String,
    pub use_mock_secret_store: bool,
}

pub enum PaymentDirection {
    Inbound,
    Outbound,
}

impl From<PaymentDirectionRs> for PaymentDirection {
    fn from(value: PaymentDirectionRs) -> Self {
        match value {
            PaymentDirectionRs::Inbound => Self::Inbound,
            PaymentDirectionRs::Outbound => Self::Outbound,
        }
    }
}

pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

impl From<PaymentStatusRs> for PaymentStatus {
    fn from(value: PaymentStatusRs) -> Self {
        match value {
            PaymentStatusRs::Pending => Self::Pending,
            PaymentStatusRs::Completed => Self::Completed,
            PaymentStatusRs::Failed => Self::Failed,
        }
    }
}

pub enum PaymentKind {
    Onchain,
    Invoice,
    Spontaneous,
}

impl From<PaymentKindRs> for PaymentKind {
    fn from(value: PaymentKindRs) -> Self {
        match value {
            PaymentKindRs::Onchain => Self::Onchain,
            PaymentKindRs::Invoice => Self::Invoice,
            PaymentKindRs::Spontaneous => Self::Spontaneous,
        }
    }
}

/// Just the info we need to display an entry in the payments list UI.
#[frb(dart_metadata=("freezed"))]
pub struct ShortPayment {
    pub index: String,
    pub kind: PaymentKind,
    pub direction: PaymentDirection,
    pub amount_sat: Option<u64>,
    pub status: PaymentStatus,
    /// This field will prioritize the `note` the user explicitly sets, over
    /// the LN invoice description which is not user controlled.
    pub note: Option<String>,
    pub created_at: i64,
}

impl From<&BasicPayment> for ShortPayment {
    fn from(payment: &BasicPayment) -> Self {
        Self {
            index: payment.index().to_string(),
            kind: PaymentKind::from(payment.kind),
            direction: PaymentDirection::from(payment.direction),
            amount_sat: payment.amount.map(|amt| amt.sats_u64()),
            status: PaymentStatus::from(payment.status),
            note: payment.note_or_description().map(|s| s.to_owned()),
            created_at: payment.created_at().as_i64(),
        }
    }
}

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
#[frb(dart_metadata=("freezed"))]
pub struct ClientPaymentId {
    pub id: [u8; 32],
}

pub fn gen_client_payment_id() -> SyncReturn<ClientPaymentId> {
    SyncReturn(ClientPaymentId {
        id: ClientPaymentIdRs::from_rng(&mut SysRng::new()).0,
    })
}

impl From<ClientPaymentId> for ClientPaymentIdRs {
    fn from(value: ClientPaymentId) -> ClientPaymentIdRs {
        ClientPaymentIdRs(value.id)
    }
}

// TODO(phlip9): error messages need to be internationalized

/// Validate whether `address_str` is a properly formatted bitcoin address. Also
/// checks that it's valid for the configured bitcoin network.
///
/// The return type is a bit funky: `Option<String>`. `None` means
/// `address_str` is valid, while `Some(msg)` means it is not (with given
/// error message). We return in this format to better match the flutter
/// `FormField` validator API.
pub fn form_validate_bitcoin_address(
    address_str: String,
    current_network: Network,
) -> SyncReturn<Option<String>> {
    let result = form::validate_bitcoin_address(
        &address_str,
        common::cli::Network::from(current_network),
    );
    SyncReturn(match result {
        Ok(()) => None,
        Err(msg) => Some(msg),
    })
}

pub enum ConfirmationPriority {
    High,
    Normal,
    Background,
}

impl From<ConfirmationPriority> for ConfirmationPriorityRs {
    fn from(value: ConfirmationPriority) -> Self {
        match value {
            ConfirmationPriority::High => Self::High,
            ConfirmationPriority::Normal => Self::Normal,
            ConfirmationPriority::Background => Self::Background,
        }
    }
}

/// The maximum allowed payment note size in bytes.
///
/// See [`common::constants::MAX_PAYMENT_NOTE_BYTES`].
pub const MAX_PAYMENT_NOTE_BYTES: usize = 512;
// Assert that these two constants are exactly equal at compile time.
const _: [(); MAX_PAYMENT_NOTE_BYTES] =
    [(); common::constants::MAX_PAYMENT_NOTE_BYTES];

fn validate_note(note: String) -> anyhow::Result<String> {
    if note.len() <= MAX_PAYMENT_NOTE_BYTES {
        Ok(note)
    } else {
        Err(anyhow!("The payment note is too long."))
    }
}

pub struct SendOnchainRequest {
    pub cid: ClientPaymentId,
    pub address: String,
    pub amount_sats: u64,
    pub priority: ConfirmationPriority,
    pub note: Option<String>,
}

impl TryFrom<SendOnchainRequest> for SendOnchainRequestRs {
    type Error = anyhow::Error;

    fn try_from(req: SendOnchainRequest) -> anyhow::Result<Self> {
        let address = bitcoin::Address::from_str(&req.address)
            .map_err(|_| anyhow!("The bitcoin address isn't valid."))?;
        let amount = Amount::try_from_sats_u64(req.amount_sats)?;

        Ok(Self {
            cid: req.cid.into(),
            address,
            amount,
            priority: req.priority.into(),
            note: req.note.map(validate_note).transpose()?,
        })
    }
}

pub struct EstimateFeeSendOnchainRequest {
    pub address: String,
    pub amount_sats: u64,
}

impl TryFrom<EstimateFeeSendOnchainRequest>
    for EstimateFeeSendOnchainRequestRs
{
    type Error = anyhow::Error;

    fn try_from(req: EstimateFeeSendOnchainRequest) -> anyhow::Result<Self> {
        let address = bitcoin::Address::from_str(&req.address)
            .map_err(|_| anyhow!("The bitcoin address isn't valid."))?;
        let amount = Amount::try_from_sats_u64(req.amount_sats)?;

        Ok(Self { address, amount })
    }
}

pub struct EstimateFeeSendOnchainResponse {
    pub high: Option<FeeEstimate>,
    pub normal: FeeEstimate,
    pub background: FeeEstimate,
}

impl From<EstimateFeeSendOnchainResponseRs> for EstimateFeeSendOnchainResponse {
    fn from(resp: EstimateFeeSendOnchainResponseRs) -> Self {
        Self {
            high: resp.high.map(FeeEstimate::from),
            normal: FeeEstimate::from(resp.normal),
            background: FeeEstimate::from(resp.background),
        }
    }
}

pub struct FeeEstimate {
    pub amount_sats: u64,
}

impl From<FeeEstimateRs> for FeeEstimate {
    fn from(value: FeeEstimateRs) -> Self {
        Self {
            amount_sats: value.amount.sats_u64(),
        }
    }
}

/// Init the Rust [`tracing`] logger. Also sets the current `RUST_LOG_TX`
/// instance, which ships Rust logs over to the dart side for printing.
///
/// Since `println!`/stdout gets swallowed on mobile, we ship log messages over
/// to dart for printing. Otherwise we can't see logs while developing.
///
/// When dart calls this function, it generates a `log_tx` and `log_rx`, then
/// sends the `log_tx` to Rust while holding on to the `log_rx`. When Rust gets
/// a new [`tracing`] log event, it enqueues the formatted log onto the
/// `log_tx`.
///
/// Unlike our other Rust loggers, this init will _not_ panic if a
/// logger instance is already set. Instead it will just update the
/// `RUST_LOG_TX`. This funky setup allows us to seamlessly support flutter's
/// hot restart, which would otherwise try to re-init the logger (and cause a
/// panic) but we still need to register a new log tx.
///
/// `rust_log`: since env vars don't work well on mobile, we need to ship the
/// equivalent of `$RUST_LOG` configured at build-time through here.
pub fn init_rust_log_stream(rust_log_tx: StreamSink<String>, rust_log: String) {
    logger::init(rust_log_tx, &rust_log);
}

/// Delete the local persisted `SecretStore` and `RootSeed`.
///
/// WARNING: you will need a backup recovery to use the account afterwards.
pub fn debug_delete_secret_store(
    config: Config,
) -> anyhow::Result<SyncReturn<()>> {
    SecretStore::new(&config.into()).delete().map(SyncReturn)
}

fn block_on<T, Fut>(future: Fut) -> T
where
    Fut: Future<Output = T>,
{
    RUNTIME.block_on(future)
}

/// The `AppHandle` is a Dart representation of an [`App`] instance.
pub struct AppHandle {
    pub inner: RustOpaque<App>,
}

impl AppHandle {
    fn new(app: App) -> Self {
        Self {
            inner: RustOpaque::new(app),
        }
    }

    pub fn load(config: Config) -> anyhow::Result<Option<AppHandle>> {
        block_on(async move {
            Ok(App::load(&mut SysRng::new(), config.into())
                .await
                .context("Failed to load saved App state")?
                .map(AppHandle::new))
        })
    }

    pub fn restore(
        config: Config,
        seed_phrase: String,
    ) -> anyhow::Result<AppHandle> {
        block_on(async move {
            App::restore(config.into(), seed_phrase)
                .await
                .context("Failed to restore from seed phrase")
                .map(Self::new)
        })
    }

    pub fn signup(config: Config) -> anyhow::Result<AppHandle> {
        block_on(async move {
            App::signup(&mut SysRng::new(), config.into())
                .await
                .context("Failed to generate and signup new wallet")
                .map(Self::new)
        })
    }

    pub fn node_info(&self) -> anyhow::Result<NodeInfo> {
        block_on(self.inner.node_client().node_info())
            .map(NodeInfo::from)
            .map_err(anyhow::Error::new)
    }

    pub fn fiat_rates(&self) -> anyhow::Result<FiatRates> {
        block_on(self.inner.gateway_client().get_fiat_rates())
            .map(FiatRates::from)
            .map_err(anyhow::Error::new)
    }

    pub fn send_onchain(&self, req: SendOnchainRequest) -> anyhow::Result<()> {
        let req = SendOnchainRequestRs::try_from(req)?;
        block_on(self.inner.node_client().send_onchain(req))
            .map(|_txid| ())
            .map_err(anyhow::Error::new)
    }

    pub fn estimate_fee_send_onchain(
        &self,
        req: EstimateFeeSendOnchainRequest,
    ) -> anyhow::Result<EstimateFeeSendOnchainResponse> {
        let req = EstimateFeeSendOnchainRequestRs::try_from(req)?;
        block_on(self.inner.node_client().estimate_fee_send_onchain(req))
            .map(EstimateFeeSendOnchainResponse::from)
            .map_err(anyhow::Error::new)
    }

    pub fn get_address(&self) -> anyhow::Result<String> {
        block_on(self.inner.node_client().get_address())
            .map(|addr| addr.to_string())
            .map_err(anyhow::Error::new)
    }

    /// Delete both the local payment state and the on-disk payment db.
    pub fn delete_payment_db(&self) -> anyhow::Result<()> {
        let mut db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.delete().context("Failed to delete PaymentDb")
    }

    /// Sync the local payment DB to the remote node.
    ///
    /// Returns `true` if any payment changed, so we know whether to reload the
    /// payment list UI.
    pub fn sync_payments(&self) -> anyhow::Result<bool> {
        block_on(self.inner.sync_payments())
            .map(|summary| summary.any_changes())
    }

    pub fn get_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> SyncReturn<Option<ShortPayment>> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(
            db_lock
                .state()
                .get_payment_by_scroll_idx(scroll_idx)
                .map(ShortPayment::from),
        )
    }

    pub fn get_pending_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> SyncReturn<Option<ShortPayment>> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(
            db_lock
                .state()
                .get_pending_payment_by_scroll_idx(scroll_idx)
                .map(ShortPayment::from),
        )
    }

    pub fn get_finalized_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> SyncReturn<Option<ShortPayment>> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(
            db_lock
                .state()
                .get_finalized_payment_by_scroll_idx(scroll_idx)
                .map(ShortPayment::from),
        )
    }

    pub fn get_num_payments(&self) -> SyncReturn<usize> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(db_lock.state().num_payments())
    }

    pub fn get_num_pending_payments(&self) -> SyncReturn<usize> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(db_lock.state().num_pending())
    }

    pub fn get_num_finalized_payments(&self) -> SyncReturn<usize> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(db_lock.state().num_finalized())
    }
}
