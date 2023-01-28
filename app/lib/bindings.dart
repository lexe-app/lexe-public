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
  // Android only supports ffi via dynamically linked libraries.
  // We'll statically link our ffi for all other platforms.
  final lib = (io.Platform.isAndroid)
      ? ffi.DynamicLibrary.open("libapp_rs.so")
      : ffi.DynamicLibrary.process();
  return lib;
}

// The instantiated Rust API. Use it like `api.hello()`.
final api = bindings_generated.AppRsImpl(loadLibrary());
