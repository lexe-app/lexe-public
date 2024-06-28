// The `ffi` module loads the native Rust API.
//
// * The Rust API is defined in `app-rs/src/ffi/ffi.rs`.
//
// * From the Dart side, see the available APIs in
//   `app/lib/ffi/ffi_generated_api.dart`.

import 'dart:ffi' as ffi;
import 'dart:io' as io;

import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/ffi/ffi_generated.dart' show AppRsImpl;

/// Android only supports ffi via dynamically linked libraries.
/// I couldn't figure out how to statically link against our lib on Linux.
/// Prefer to statically link our ffi library for all platforms.
ffi.DynamicLibrary _loadLibraryNormal() {
  final lib = (io.Platform.isAndroid || io.Platform.isLinux)
      ? ffi.DynamicLibrary.open("libapp_rs.so")
      : ffi.DynamicLibrary.process();
  return lib;
}

/// Unit tests are run on the host and `flutter test` (with unit test only)
/// just straight up runs `dart run` on the test file without giving us any
/// build hooks or letting us link our library, so we have to load the dynamic
/// library out of the cargo target dir. Assumes we've just run
/// `cargo build -p app-rs` just before.
ffi.DynamicLibrary _loadLibraryUnitTest() {
  if (io.Platform.isMacOS) {
    return ffi.DynamicLibrary.open("../target/debug/libapp_rs.dylib");
  } else if (io.Platform.isLinux) {
    return ffi.DynamicLibrary.open("../target/debug/libapp_rs.so");
  } else {
    throw UnsupportedError("Unsupported unit test platform");
  }
}

/// Load the app-rs Rust FFI library.
ffi.DynamicLibrary _loadLibrary() {
  if (cfg.debug && cfg.test) {
    return _loadLibraryUnitTest();
  } else {
    return _loadLibraryNormal();
  }
}

// The instantiated Rust API. Use it like `api.hello()`.
final AppRsImpl api = AppRsImpl(_loadLibrary());
