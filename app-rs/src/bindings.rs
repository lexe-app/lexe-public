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
//!
//! ## Important implementation details
//!
//! In an ideal world, we'd have an `Arc<App>` handle or something that can
//! be passed around in the Dart code as an opaque pointer.
//!
//! Unfortunately, the current state of Rust<->Dart FFI doesn't make it
//! particularly safe or ergonomic to pass _opaque_ handles across the boundary.
//! Rather the bindings feel best when all data is _copied_ across.
//!
//! Our current approach then is to use... globals for long-lived state in the
//! Rust code.
//!
//! Since Dart tests appear to run serially (?), this might not be too much of
//! an issue, since we can just drop and reset the global state between each
//! test.

use std::future::Future;
use std::sync::OnceLock;

use anyhow::Context;
use flutter_rust_bridge::SyncReturn;

use crate::app::App;

// TODO(phlip9): remove hello*

pub fn hello() -> SyncReturn<String> {
    SyncReturn("hello!".to_string())
}

pub fn hello_async() -> String {
    "hello!".to_string()
}

// TODO: explain this
thread_local! {
    static RUNTIME: tokio::runtime::Runtime
        = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build thread's tokio Runtime");
}

// see top module comment
static APP: OnceLock<App> = OnceLock::new();

pub enum BuildVariant {
    Production,
    Staging,
    Development,
}

pub enum Network {
    Bitcoin,
    Testnet,
    Regtest,
}

pub struct Config {
    pub build_variant: BuildVariant,
    pub network: Network,
}

impl Config {
    pub fn regtest() -> SyncReturn<Config> {
        SyncReturn(Config {
            build_variant: BuildVariant::Development,
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

// APPROACH 1: plain static functions

fn assert_app_not_loaded() {
    if APP.get().is_some() {
        panic!("APP instance is already set!");
    }
}

fn set_app_instance(app: App) {
    if APP.set(app).is_err() {
        panic!("APP instance was set while we were loading/signing up!");
    }
}

pub fn app_load(config: Config) -> anyhow::Result<bool> {
    block_on(async move {
        assert_app_not_loaded();
        App::load(config)
            .await
            .context("Failed to load saved App state")
            .map(|maybe_app: Option<App>| {
                maybe_app.map(set_app_instance).is_some()
            })
    })
}

pub fn app_recover(config: Config, seed_phrase: String) -> anyhow::Result<()> {
    block_on(async move {
        assert_app_not_loaded();
        App::recover(config, seed_phrase)
            .await
            .context("Failed to recover from seed phrase")
            .map(set_app_instance)
    })
}

pub fn app_signup(config: Config) -> anyhow::Result<()> {
    block_on(async move {
        assert_app_not_loaded();
        App::signup(config)
            .await
            .context("Failed to generate and signup new wallet")
            .map(set_app_instance)
    })
}

// APPROACH 2: dart code holds handle which has methods

/// The `AppHandle` is a Dart representation of a current [`App`] instance.
pub struct AppHandle {
    pub instance_id: i32,
}

impl AppHandle {
    fn assert_no_instance() {
        if APP.get().is_some() {
            panic!("APP instance is already set!");
        }
    }

    fn set_instance(app: App) -> Self {
        let instance_id = app.instance_id();
        if APP.set(app).is_err() {
            panic!("APP instance was set while we were loading/signing up!");
        }
        Self { instance_id }
    }

    fn instance(&self) -> &'static App {
        let app = APP.get().expect("There is no loaded APP instance yet!");
        assert_eq!(app.instance_id(), self.instance_id);
        app
    }

    // TODO(phlip9): dummy method to test method codegen. remove.
    pub fn test_method(&self) -> anyhow::Result<()> {
        self.instance().test_method()
    }

    pub fn load(config: Config) -> anyhow::Result<Option<AppHandle>> {
        Self::assert_no_instance();

        block_on(async move {
            App::load(config)
                .await
                .context("Failed to load saved App state")
                .map(|maybe_app: Option<App>| maybe_app.map(Self::set_instance))
        })
    }

    pub fn recover(
        config: Config,
        seed_phrase: String,
    ) -> anyhow::Result<AppHandle> {
        Self::assert_no_instance();

        block_on(async move {
            App::recover(config, seed_phrase)
                .await
                .context("Failed to recover from seed phrase")
                .map(Self::set_instance)
        })
    }

    pub fn signup(config: Config) -> anyhow::Result<AppHandle> {
        Self::assert_no_instance();

        block_on(async move {
            App::signup(config)
                .await
                .context("Failed to generate and signup new wallet")
                .map(Self::set_instance)
        })
    }
}
