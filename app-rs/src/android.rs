//! Binding code to interface with Android's JVM instance and the current global
//! Android [Context](https://developer.android.com/reference/android/content/Context).

use std::{ptr, sync::OnceLock};

use anyhow::Context;
use jni::{
    objects::{GlobalRef, JClass, JObject, JString},
    sys::jstring,
    JNIEnv, JavaVM,
};

struct AndroidContext {
    /// A handle to the JVM. Lets us call into Java/Kotlin from Rust.
    #[allow(dead_code)] // TODO(phlip9): remove
    jvm: JavaVM,

    /// The global Android [Context](https://developer.android.com/reference/android/content/Context)
    /// for the current process. Lets us access Android services from Rust.
    #[allow(dead_code)] // TODO(phlip9): remove
    context: GlobalRef,
}

/// The global Android context.
static ANDROID_CONTEXT: OnceLock<AndroidContext> = OnceLock::new();

/// Can't unwind across an FFI boundary. Just abort if we somehow fail to make
/// a string...
fn new_jstring_or_abort(env: &JNIEnv, s: String) -> jstring {
    env.new_string(s)
        .map(JString::into_raw)
        .unwrap_or_else(|_| std::process::abort())
}

/// Called when `MainActivity.onCreate` is called. Here we can get a handle to
/// the JVM and Android `Context`.
///
/// See [MainActivity.onCreateNative](../../app/android/app/src/main/kotlin/app/
/// lexe/lexeapp/MainActivity.kt)
#[no_mangle]
pub extern "C" fn Java_app_lexe_lexeapp_MainActivity_onCreateNative(
    env: JNIEnv,
    _class: JClass,
    context: JObject,
) -> jstring {
    on_create_native(&env, context)
        .map(|()| ptr::null_mut())
        .unwrap_or_else(|err| {
            new_jstring_or_abort(&env, format!("onCreateNative: {err:#}"))
        })
}

fn on_create_native(env: &JNIEnv, context: JObject) -> anyhow::Result<()> {
    let jvm = env.get_java_vm().context("Failed to get JVM handle")?;
    let context = env
        .new_global_ref(context)
        .context("Failed to get Android Context handle")?;

    // Store context
    let _ = ANDROID_CONTEXT.set(AndroidContext { jvm, context });

    Ok(())
}
