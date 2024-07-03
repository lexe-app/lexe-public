// The `ffi` module loads the native Rust API.
//
// * The Rust API is defined in `app-rs/src/ffi/ffi.rs`.
//
// * From the Dart side, see the available APIs in
//   `app/lib/app_rs/ffi/ffi.dart`.

import 'dart:io' as io;

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:lexeapp/cfg.dart' as cfg;

/// Android only supports ffi via dynamically linked libraries.
/// I couldn't figure out how to statically link against our lib on Linux.
/// Prefer to statically link our ffi library for all platforms.
ExternalLibrary _loadLibraryNormal() {
  final lib = (io.Platform.isAndroid || io.Platform.isLinux)
      ? ExternalLibrary.open("libapp_rs.so")
      : ExternalLibrary.process(
          // If we ever have other external dart dependencies that also use
          // flutter_rust_bridge, we'll have to make sure we load them as *.so
          // dynamic libraries (the default).
          iKnowHowToUseIt: true,
        );
  return lib;
}

/// Unit tests are run on the host and `flutter test` (with unit test only)
/// just straight up runs `dart run` on the test file without giving us any
/// build hooks or letting us link our library, so we have to load the dynamic
/// library out of the cargo target dir. Assumes we've just run
/// `cargo build -p app-rs` just before.
ExternalLibrary _loadLibraryUnitTest() {
  if (io.Platform.isMacOS) {
    return ExternalLibrary.open("../target/debug/libapp_rs.dylib");
  } else if (io.Platform.isLinux) {
    return ExternalLibrary.open("../target/debug/libapp_rs.so");
  } else {
    throw UnsupportedError("Unsupported unit test platform");
  }
}

/// Load the app-rs Rust FFI library.
ExternalLibrary _loadLibrary() {
  if (cfg.debug && cfg.test) {
    return _loadLibraryUnitTest();
  } else {
    return _loadLibraryNormal();
  }
}

/// `app-rs` needs to be loaded either  as a shared library or already
/// statically linked depending on the platform.
final ExternalLibrary appRsLib = _loadLibrary();
