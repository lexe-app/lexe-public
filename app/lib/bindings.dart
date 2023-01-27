// The `bindings` module loads the native Rust API.
//
// * The Rust API is defined in `app-rs/src/bindings.rs`.
//
// * From the Dart side, see the available APIs in
//   `app/lib/bindings_generated_api.dart`.

import 'dart:ffi' as ffi;
import 'dart:io' as io;

import 'bindings_generated.dart' as bindings_generated;

ffi.DynamicLibrary loadLibrary() {
  // To load the native library, iOS uses static linking while Android
  // (and others) use dynamic linking.
  final dylib = (io.Platform.isIOS || io.Platform.isMacOS)
      ? ffi.DynamicLibrary.process()
      : ffi.DynamicLibrary.open("libapp_rs.so");
  return dylib;
}

// The instantiated Rust API. Use it like `api.hello()`.
final api = bindings_generated.AppRsImpl(loadLibrary());
