#![allow(
    non_camel_case_types,
    unused,
    clippy::redundant_closure,
    clippy::useless_conversion,
    clippy::unit_arg,
    clippy::double_parens,
    non_snake_case,
    clippy::too_many_arguments
)]
// AUTO GENERATED FILE, DO NOT EDIT.
// Generated by `flutter_rust_bridge`@ 1.75.1.

use core::panic::UnwindSafe;
use std::{ffi::c_void, sync::Arc};

use flutter_rust_bridge::*;

use crate::bindings::*;

// Section: imports

// Section: wire functions

fn wire_init_rust_log_stream_impl(
    port_: MessagePort,
    rust_log: impl Wire2Api<String> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "init_rust_log_stream",
            port: Some(port_),
            mode: FfiCallMode::Stream,
        },
        move || {
            let api_rust_log = rust_log.wire2api();
            move |task_callback| {
                Ok(init_rust_log_stream(
                    task_callback.stream_sink(),
                    api_rust_log,
                ))
            }
        },
    )
}
fn wire_load__static_method__AppHandle_impl(
    port_: MessagePort,
    config: impl Wire2Api<Config> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "load__static_method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_config = config.wire2api();
            move |task_callback| AppHandle::load(api_config)
        },
    )
}
fn wire_restore__static_method__AppHandle_impl(
    port_: MessagePort,
    config: impl Wire2Api<Config> + UnwindSafe,
    seed_phrase: impl Wire2Api<String> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "restore__static_method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_config = config.wire2api();
            let api_seed_phrase = seed_phrase.wire2api();
            move |task_callback| AppHandle::restore(api_config, api_seed_phrase)
        },
    )
}
fn wire_signup__static_method__AppHandle_impl(
    port_: MessagePort,
    config: impl Wire2Api<Config> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "signup__static_method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_config = config.wire2api();
            move |task_callback| AppHandle::signup(api_config)
        },
    )
}
fn wire_node_info__method__AppHandle_impl(
    port_: MessagePort,
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "node_info__method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_that = that.wire2api();
            move |task_callback| AppHandle::node_info(&api_that)
        },
    )
}
fn wire_fiat_rates__method__AppHandle_impl(
    port_: MessagePort,
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "fiat_rates__method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_that = that.wire2api();
            move |task_callback| AppHandle::fiat_rates(&api_that)
        },
    )
}
fn wire_sync_payments__method__AppHandle_impl(
    port_: MessagePort,
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap(
        WrapInfo {
            debug_name: "sync_payments__method__AppHandle",
            port: Some(port_),
            mode: FfiCallMode::Normal,
        },
        move || {
            let api_that = that.wire2api();
            move |task_callback| AppHandle::sync_payments(&api_that)
        },
    )
}
fn wire_get_payment_by_scroll_idx__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
    scroll_idx: impl Wire2Api<usize> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name: "get_payment_by_scroll_idx__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            let api_scroll_idx = scroll_idx.wire2api();
            Ok(AppHandle::get_payment_by_scroll_idx(
                &api_that,
                api_scroll_idx,
            ))
        },
    )
}
fn wire_get_pending_payment_by_scroll_idx__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
    scroll_idx: impl Wire2Api<usize> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name: "get_pending_payment_by_scroll_idx__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            let api_scroll_idx = scroll_idx.wire2api();
            Ok(AppHandle::get_pending_payment_by_scroll_idx(
                &api_that,
                api_scroll_idx,
            ))
        },
    )
}
fn wire_get_finalized_payment_by_scroll_idx__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
    scroll_idx: impl Wire2Api<usize> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name:
                "get_finalized_payment_by_scroll_idx__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            let api_scroll_idx = scroll_idx.wire2api();
            Ok(AppHandle::get_finalized_payment_by_scroll_idx(
                &api_that,
                api_scroll_idx,
            ))
        },
    )
}
fn wire_get_num_payments__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name: "get_num_payments__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            Ok(AppHandle::get_num_payments(&api_that))
        },
    )
}
fn wire_get_num_pending_payments__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name: "get_num_pending_payments__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            Ok(AppHandle::get_num_pending_payments(&api_that))
        },
    )
}
fn wire_get_num_finalized_payments__method__AppHandle_impl(
    that: impl Wire2Api<AppHandle> + UnwindSafe,
) -> support::WireSyncReturn {
    FLUTTER_RUST_BRIDGE_HANDLER.wrap_sync(
        WrapInfo {
            debug_name: "get_num_finalized_payments__method__AppHandle",
            port: None,
            mode: FfiCallMode::Sync,
        },
        move || {
            let api_that = that.wire2api();
            Ok(AppHandle::get_num_finalized_payments(&api_that))
        },
    )
}
// Section: wrapper structs

// Section: static checks

// Section: allocate functions

// Section: related functions

// Section: impl Wire2Api

pub trait Wire2Api<T> {
    fn wire2api(self) -> T;
}

impl<T, S> Wire2Api<Option<T>> for *mut S
where
    *mut S: Wire2Api<T>,
{
    fn wire2api(self) -> Option<T> {
        (!self.is_null()).then(|| self.wire2api())
    }
}

impl Wire2Api<bool> for bool {
    fn wire2api(self) -> bool {
        self
    }
}

impl Wire2Api<DeployEnv> for i32 {
    fn wire2api(self) -> DeployEnv {
        match self {
            0 => DeployEnv::Prod,
            1 => DeployEnv::Staging,
            2 => DeployEnv::Dev,
            _ => unreachable!("Invalid variant for DeployEnv: {}", self),
        }
    }
}
impl Wire2Api<i32> for i32 {
    fn wire2api(self) -> i32 {
        self
    }
}
impl Wire2Api<Network> for i32 {
    fn wire2api(self) -> Network {
        match self {
            0 => Network::Bitcoin,
            1 => Network::Testnet,
            2 => Network::Regtest,
            _ => unreachable!("Invalid variant for Network: {}", self),
        }
    }
}
impl Wire2Api<u8> for u8 {
    fn wire2api(self) -> u8 {
        self
    }
}

impl Wire2Api<usize> for usize {
    fn wire2api(self) -> usize {
        self
    }
}
// Section: impl IntoDart

impl support::IntoDart for AppHandle {
    fn into_dart(self) -> support::DartAbi {
        vec![self.inner.into_dart()].into_dart()
    }
}
impl support::IntoDartExceptPrimitive for AppHandle {}

impl support::IntoDart for FiatRate {
    fn into_dart(self) -> support::DartAbi {
        vec![self.fiat.into_dart(), self.rate.into_dart()].into_dart()
    }
}
impl support::IntoDartExceptPrimitive for FiatRate {}

impl support::IntoDart for FiatRates {
    fn into_dart(self) -> support::DartAbi {
        vec![self.timestamp_ms.into_dart(), self.rates.into_dart()].into_dart()
    }
}
impl support::IntoDartExceptPrimitive for FiatRates {}

impl support::IntoDart for NodeInfo {
    fn into_dart(self) -> support::DartAbi {
        vec![
            self.node_pk.into_dart(),
            self.local_balance_msat.into_dart(),
        ]
        .into_dart()
    }
}
impl support::IntoDartExceptPrimitive for NodeInfo {}

impl support::IntoDart for PaymentDirection {
    fn into_dart(self) -> support::DartAbi {
        match self {
            Self::Inbound => 0,
            Self::Outbound => 1,
        }
        .into_dart()
    }
}
impl support::IntoDartExceptPrimitive for PaymentDirection {}
impl support::IntoDart for PaymentKind {
    fn into_dart(self) -> support::DartAbi {
        match self {
            Self::Onchain => 0,
            Self::Invoice => 1,
            Self::Spontaneous => 2,
        }
        .into_dart()
    }
}
impl support::IntoDartExceptPrimitive for PaymentKind {}
impl support::IntoDart for PaymentStatus {
    fn into_dart(self) -> support::DartAbi {
        match self {
            Self::Pending => 0,
            Self::Completed => 1,
            Self::Failed => 2,
        }
        .into_dart()
    }
}
impl support::IntoDartExceptPrimitive for PaymentStatus {}
impl support::IntoDart for ShortPayment {
    fn into_dart(self) -> support::DartAbi {
        vec![
            self.index.into_dart(),
            self.kind.into_dart(),
            self.direction.into_dart(),
            self.amount_sat.into_dart(),
            self.status.into_dart(),
            self.note.into_dart(),
            self.created_at.into_dart(),
        ]
        .into_dart()
    }
}
impl support::IntoDartExceptPrimitive for ShortPayment {}

// Section: executor

/* nothing since executor detected */

#[cfg(not(target_family = "wasm"))]
mod io {
    use super::*;
    // Section: wire functions

    #[no_mangle]
    pub extern "C" fn wire_init_rust_log_stream(
        port_: i64,
        rust_log: *mut wire_uint_8_list,
    ) {
        wire_init_rust_log_stream_impl(port_, rust_log)
    }

    #[no_mangle]
    pub extern "C" fn wire_load__static_method__AppHandle(
        port_: i64,
        config: *mut wire_Config,
    ) {
        wire_load__static_method__AppHandle_impl(port_, config)
    }

    #[no_mangle]
    pub extern "C" fn wire_restore__static_method__AppHandle(
        port_: i64,
        config: *mut wire_Config,
        seed_phrase: *mut wire_uint_8_list,
    ) {
        wire_restore__static_method__AppHandle_impl(port_, config, seed_phrase)
    }

    #[no_mangle]
    pub extern "C" fn wire_signup__static_method__AppHandle(
        port_: i64,
        config: *mut wire_Config,
    ) {
        wire_signup__static_method__AppHandle_impl(port_, config)
    }

    #[no_mangle]
    pub extern "C" fn wire_node_info__method__AppHandle(
        port_: i64,
        that: *mut wire_AppHandle,
    ) {
        wire_node_info__method__AppHandle_impl(port_, that)
    }

    #[no_mangle]
    pub extern "C" fn wire_fiat_rates__method__AppHandle(
        port_: i64,
        that: *mut wire_AppHandle,
    ) {
        wire_fiat_rates__method__AppHandle_impl(port_, that)
    }

    #[no_mangle]
    pub extern "C" fn wire_sync_payments__method__AppHandle(
        port_: i64,
        that: *mut wire_AppHandle,
    ) {
        wire_sync_payments__method__AppHandle_impl(port_, that)
    }

    #[no_mangle]
    pub extern "C" fn wire_get_payment_by_scroll_idx__method__AppHandle(
        that: *mut wire_AppHandle,
        scroll_idx: usize,
    ) -> support::WireSyncReturn {
        wire_get_payment_by_scroll_idx__method__AppHandle_impl(that, scroll_idx)
    }

    #[no_mangle]
    pub extern "C" fn wire_get_pending_payment_by_scroll_idx__method__AppHandle(
        that: *mut wire_AppHandle,
        scroll_idx: usize,
    ) -> support::WireSyncReturn {
        wire_get_pending_payment_by_scroll_idx__method__AppHandle_impl(
            that, scroll_idx,
        )
    }

    #[no_mangle]
    pub extern "C" fn wire_get_finalized_payment_by_scroll_idx__method__AppHandle(
        that: *mut wire_AppHandle,
        scroll_idx: usize,
    ) -> support::WireSyncReturn {
        wire_get_finalized_payment_by_scroll_idx__method__AppHandle_impl(
            that, scroll_idx,
        )
    }

    #[no_mangle]
    pub extern "C" fn wire_get_num_payments__method__AppHandle(
        that: *mut wire_AppHandle,
    ) -> support::WireSyncReturn {
        wire_get_num_payments__method__AppHandle_impl(that)
    }

    #[no_mangle]
    pub extern "C" fn wire_get_num_pending_payments__method__AppHandle(
        that: *mut wire_AppHandle,
    ) -> support::WireSyncReturn {
        wire_get_num_pending_payments__method__AppHandle_impl(that)
    }

    #[no_mangle]
    pub extern "C" fn wire_get_num_finalized_payments__method__AppHandle(
        that: *mut wire_AppHandle,
    ) -> support::WireSyncReturn {
        wire_get_num_finalized_payments__method__AppHandle_impl(that)
    }

    // Section: allocate functions

    #[no_mangle]
    pub extern "C" fn new_App() -> wire_App {
        wire_App::new_with_null_ptr()
    }

    #[no_mangle]
    pub extern "C" fn new_box_autoadd_app_handle_0() -> *mut wire_AppHandle {
        support::new_leak_box_ptr(wire_AppHandle::new_with_null_ptr())
    }

    #[no_mangle]
    pub extern "C" fn new_box_autoadd_config_0() -> *mut wire_Config {
        support::new_leak_box_ptr(wire_Config::new_with_null_ptr())
    }

    #[no_mangle]
    pub extern "C" fn new_uint_8_list_0(len: i32) -> *mut wire_uint_8_list {
        let ans = wire_uint_8_list {
            ptr: support::new_leak_vec_ptr(Default::default(), len),
            len,
        };
        support::new_leak_box_ptr(ans)
    }

    // Section: related functions

    #[no_mangle]
    pub extern "C" fn drop_opaque_App(ptr: *const c_void) {
        unsafe {
            Arc::<App>::decrement_strong_count(ptr as _);
        }
    }

    #[no_mangle]
    pub extern "C" fn share_opaque_App(ptr: *const c_void) -> *const c_void {
        unsafe {
            Arc::<App>::increment_strong_count(ptr as _);
            ptr
        }
    }

    // Section: impl Wire2Api

    impl Wire2Api<RustOpaque<App>> for wire_App {
        fn wire2api(self) -> RustOpaque<App> {
            unsafe { support::opaque_from_dart(self.ptr as _) }
        }
    }
    impl Wire2Api<String> for *mut wire_uint_8_list {
        fn wire2api(self) -> String {
            let vec: Vec<u8> = self.wire2api();
            String::from_utf8_lossy(&vec).into_owned()
        }
    }
    impl Wire2Api<AppHandle> for wire_AppHandle {
        fn wire2api(self) -> AppHandle {
            AppHandle {
                inner: self.inner.wire2api(),
            }
        }
    }

    impl Wire2Api<AppHandle> for *mut wire_AppHandle {
        fn wire2api(self) -> AppHandle {
            let wrap = unsafe { support::box_from_leak_ptr(self) };
            Wire2Api::<AppHandle>::wire2api(*wrap).into()
        }
    }
    impl Wire2Api<Config> for *mut wire_Config {
        fn wire2api(self) -> Config {
            let wrap = unsafe { support::box_from_leak_ptr(self) };
            Wire2Api::<Config>::wire2api(*wrap).into()
        }
    }
    impl Wire2Api<Config> for wire_Config {
        fn wire2api(self) -> Config {
            Config {
                deploy_env: self.deploy_env.wire2api(),
                network: self.network.wire2api(),
                gateway_url: self.gateway_url.wire2api(),
                use_sgx: self.use_sgx.wire2api(),
                app_data_dir: self.app_data_dir.wire2api(),
                use_mock_secret_store: self.use_mock_secret_store.wire2api(),
            }
        }
    }

    impl Wire2Api<Vec<u8>> for *mut wire_uint_8_list {
        fn wire2api(self) -> Vec<u8> {
            unsafe {
                let wrap = support::box_from_leak_ptr(self);
                support::vec_from_leak_ptr(wrap.ptr, wrap.len)
            }
        }
    }

    // Section: wire structs

    #[repr(C)]
    #[derive(Clone)]
    pub struct wire_App {
        ptr: *const core::ffi::c_void,
    }

    #[repr(C)]
    #[derive(Clone)]
    pub struct wire_AppHandle {
        inner: wire_App,
    }

    #[repr(C)]
    #[derive(Clone)]
    pub struct wire_Config {
        deploy_env: i32,
        network: i32,
        gateway_url: *mut wire_uint_8_list,
        use_sgx: bool,
        app_data_dir: *mut wire_uint_8_list,
        use_mock_secret_store: bool,
    }

    #[repr(C)]
    #[derive(Clone)]
    pub struct wire_uint_8_list {
        ptr: *mut u8,
        len: i32,
    }

    // Section: impl NewWithNullPtr

    pub trait NewWithNullPtr {
        fn new_with_null_ptr() -> Self;
    }

    impl<T> NewWithNullPtr for *mut T {
        fn new_with_null_ptr() -> Self {
            std::ptr::null_mut()
        }
    }

    impl NewWithNullPtr for wire_App {
        fn new_with_null_ptr() -> Self {
            Self {
                ptr: core::ptr::null(),
            }
        }
    }

    impl NewWithNullPtr for wire_AppHandle {
        fn new_with_null_ptr() -> Self {
            Self {
                inner: wire_App::new_with_null_ptr(),
            }
        }
    }

    impl Default for wire_AppHandle {
        fn default() -> Self {
            Self::new_with_null_ptr()
        }
    }

    impl NewWithNullPtr for wire_Config {
        fn new_with_null_ptr() -> Self {
            Self {
                deploy_env: Default::default(),
                network: Default::default(),
                gateway_url: core::ptr::null_mut(),
                use_sgx: Default::default(),
                app_data_dir: core::ptr::null_mut(),
                use_mock_secret_store: Default::default(),
            }
        }
    }

    impl Default for wire_Config {
        fn default() -> Self {
            Self::new_with_null_ptr()
        }
    }

    // Section: sync execution mode utility

    #[no_mangle]
    pub extern "C" fn free_WireSyncReturn(ptr: support::WireSyncReturn) {
        unsafe {
            let _ = support::box_from_leak_ptr(ptr);
        };
    }
}
#[cfg(not(target_family = "wasm"))]
pub use io::*;
