// The `bindings` module loads the native Rust API.
//
// * The Rust API is defined in `app-rs/src/bindings.rs`.
//
// * From the Dart side, see the available APIs in
//   `app/lib/bindings_generated_api.dart`.

import 'dart:ffi' as ffi;
import 'dart:io' as io;

import 'bindings_generated.dart' show AppRsImpl;
import 'cfg.dart' as cfg;

/// Android only supports ffi via dynamically linked libraries.
/// We'll statically link our ffi library for all other platforms.
ffi.DynamicLibrary _loadLibraryNormal() {
  // TODO(phlip9): Linux build
  if (io.Platform.isLinux) {
    return ffi.DynamicLibrary.open("../target/debug/libapp_rs.so");
  }

  final lib = (io.Platform.isAndroid)
      ? ffi.DynamicLibrary.open("libapp_rs.so")
      : ffi.DynamicLibrary.process();
  return lib;
}

/// Unit tests are run on the host and `flutter test` (with unit test only)
/// just straight up runs `dart run` on the test file without giving us any
/// build hooks or letting us link our library, so we have to load the dynamic
/// library out of the cargo target dir.
//
// TODO(phlip9): how can we ensure the native library is always up-to-date?
// since this is run on the host, maybe we literally just shell out to a quick
// `cargo build -p app-rs`??
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
