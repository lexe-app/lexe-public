//! # Rust/Dart ffi bindings
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
//! This crate's `build.rs` runs when this file changes. It then delegates to
//! `flutter_rust_bridge_codegen` to actually generate the binding code.
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

use std::future::Future;

use anyhow::Context;
use common::rng::SysRng;
use flutter_rust_bridge::{RustOpaque, SyncReturn};

pub use crate::app::App;

// As a temporary unblock to support async fn's, we'll just block_on on a
// thread-local current_thread runtime in each worker thread.
//
// This means we can only have max 4 top-level async fns running at once before
// we block the main UI thread.
thread_local! {
    static RUNTIME: tokio::runtime::Runtime
        = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build thread's tokio Runtime");
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

    // TODO(phlip9): dummy method to test method codegen. remove.
    pub fn test_method(&self) -> anyhow::Result<()> {
        self.inner.test_method()
    }
}
