// This file is automatically generated, so please do not edit it.
// Generated by `flutter_rust_bridge`@ 2.0.0.

// Section: imports

use flutter_rust_bridge::{
    for_generated::{
        byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt},
        transform_result_dco, Lifetimeable, Lockable,
    },
    Handler, IntoIntoDart,
};

use super::*;
use crate::ffi::ffi::*;

// Section: boilerplate

flutter_rust_bridge::frb_generated_boilerplate_io!();

#[no_mangle]
pub extern "C" fn frbgen_lexeapp_rust_arc_increment_strong_count_RustOpaque_App(
    ptr: *const std::ffi::c_void,
) {
    unsafe {
        StdArc::<App>::increment_strong_count(ptr as _);
    }
}

#[no_mangle]
pub extern "C" fn frbgen_lexeapp_rust_arc_decrement_strong_count_RustOpaque_App(
    ptr: *const std::ffi::c_void,
) {
    unsafe {
        StdArc::<App>::decrement_strong_count(ptr as _);
    }
}
