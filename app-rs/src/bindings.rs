//! # Rust/Dart FFI bindings
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

use std::{
    future::Future,
    sync::{Arc, LazyLock},
};

use anyhow::Context;
pub use common::ln::payments::BasicPayment as BasicPaymentRs;
use common::{
    api::{
        command::NodeInfo as NodeInfoRs,
        def::{AppGatewayApi, AppNodeRunApi},
        fiat_rates::FiatRates as FiatRatesRs,
    },
    rng::SysRng,
};
use flutter_rust_bridge::{
    frb, handler::ReportDartErrorHandler, RustOpaque, StreamSink, SyncReturn,
};

pub use crate::app::App;
use crate::{
    dart_task_handler::{LxExecutor, LxHandler},
    logger,
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
        LxHandler::new(LxExecutor::new(error_handler), error_handler)
    });

#[frb(dart_metadata=("freezed"))]
pub struct NodeInfo {
    pub node_pk: String,
    pub local_balance_msat: u64,
}

impl From<NodeInfoRs> for NodeInfo {
    fn from(info: NodeInfoRs) -> Self {
        Self {
            node_pk: info.node_pk.to_string(),
            local_balance_msat: info.local_balance.msat(),
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

#[derive(Debug)]
pub enum Network {
    Bitcoin,
    Testnet,
    Regtest,
}

/// Dart-serializable configuration we get from the flutter side.
#[frb(dart_metadata=("freezed"))]
pub struct Config {
    pub deploy_env: DeployEnv,
    pub network: Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub app_data_dir: String,
    pub use_mock_secret_store: bool,
}

pub struct BasicPayment {
    pub inner: RustOpaque<BasicPaymentRs>,
}

impl BasicPayment {
    fn new(value: Arc<BasicPaymentRs>) -> Self {
        Self {
            inner: RustOpaque::from(value),
        }
    }

    pub fn payment_index(&self) -> SyncReturn<String> {
        SyncReturn(self.inner.index().to_string())
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
    ) -> SyncReturn<Option<BasicPayment>> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(
            db_lock
                .get_payment_by_scroll_idx(scroll_idx)
                .map(BasicPayment::new),
        )
    }

    pub fn get_num_payments(&self) -> SyncReturn<usize> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        SyncReturn(db_lock.num_payments())
    }
}
