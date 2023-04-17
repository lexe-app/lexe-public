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
//! * Normal looking pub functions, like `pub fn foo() -> u32 { 123 }` look like
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

use std::future::Future;
use std::sync::LazyLock;

use anyhow::Context;
use common::api::command::NodeInfo as NodeInfoRs;
use common::api::def::{AppGatewayApi, AppNodeRunApi};
use common::api::fiat_rates::FiatRates as FiatRatesRs;
use common::rng::SysRng;
use flutter_rust_bridge::handler::ReportDartErrorHandler;
use flutter_rust_bridge::{frb, RustOpaque, SyncReturn};

pub use crate::app::App;
use crate::dart_task_handler::{LxExecutor, LxHandler};

// TODO(phlip9): land tokio support in flutter_rust_bridge
// As a temporary unblock to support async fn's, we'll just block_on on a
// thread-local current_thread runtime in each worker thread.
//
// This means we can only have max 4 top-level async fns running at once before
// we block the main UI thread (flutter_rust_bridge defaults to 4 worker
// threads in its threadpool).
thread_local! {
    static RUNTIME: tokio::runtime::Runtime
        = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build thread's tokio Runtime");
}

pub(crate) static FLUTTER_RUST_BRIDGE_HANDLER: LazyLock<LxHandler> =
    LazyLock::new(|| {
        std::env::set_var("RUST_BACKTRACE", "1");

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

#[derive(Debug, PartialEq, Eq)]
pub enum DeployEnv {
    Prod,
    Staging,
    Dev,
}

#[derive(Debug)]
pub enum Network {
    Bitcoin,
    Testnet,
    Regtest,
}

/// Dart-serializable configuration we get from the flutter side.
pub struct Config {
    pub deploy_env: DeployEnv,
    pub network: Network,
}

impl Config {
    pub fn regtest() -> SyncReturn<Config> {
        SyncReturn(Config {
            deploy_env: DeployEnv::Dev,
            network: Network::Regtest,
        })
    }
}

fn block_on<T, Fut>(future: Fut) -> T
where
    Fut: Future<Output = T>,
{
    RUNTIME.with(|rt| rt.block_on(future))
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
            Ok(App::load(config.into())
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
}

// for testing that backtraces are generated...

pub fn do_panic_sync() -> SyncReturn<()> {
    panic!("this should panic");
}

pub fn do_panic_async() {
    panic!("this should panic");
}

pub fn do_return_err_sync() -> anyhow::Result<SyncReturn<String>> {
    Err(anyhow::format_err!("oh no!"))
}

pub fn do_return_err_async() -> anyhow::Result<String> {
    Err(anyhow::format_err!("oh no!"))
}
