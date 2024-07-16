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

/// I'd really prefer to statically link `app-rs` everywhere, but
/// flutter_rust_bridge 2.0 currently makes this very difficult, since it no
/// longer dumps the actually-used symbols for anti-stripping mitigation.
///
/// So for now we're going dynamically linked everywhere...
ExternalLibrary _loadLibraryNormal() {
  if (io.Platform.isAndroid || io.Platform.isLinux) {
    return ExternalLibrary.open("libapp_rs.so");
  } else if (io.Platform.isIOS || io.Platform.isMacOS) {
    return ExternalLibrary.open("app_rs.dylib");
  } else {
    throw UnsupportedError(
        "Unsupported platform. Don't know how to load app-rs shared library.");
  }
}

/// Unit tests are run on the host and `flutter test` (with unit test only)
/// just straight up runs `dart run` on the test file without giving us any
/// build hooks or letting us link our library, so we have to load the dynamic
/// library out of the cargo target dir. Assumes we've just run
/// `cargo build -p app-rs` just before.
ExternalLibrary _loadLibraryUnitTest() {
  if (io.Platform.isMacOS) {
    // cargo outputs `libapp_rs.dylib` by default. It's cargo-xcode + lipo that
    // renames it to `app_rs.dylib`.
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
