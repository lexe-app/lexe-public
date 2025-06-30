// The `ffi` module loads the native Rust API.
//
// * The Rust API is defined in `app-rs/src/ffi/ffi.rs`.
//
// * From the Dart side, see the available APIs in
//   `app_rs_dart/lib/ffi/ffi.dart`.

import 'dart:io' as io;

import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;

/// `true` when the flutter app is built in debug mode with debugging info and
/// debug symbols built-in (i.e., not profile or release mode).
///
/// Since this is a constant, the dart compiler can eliminate unreachable,
/// debug-only blocks.
const bool _cfgDebug = kDebugMode;

/// `true` only in unit tests. This env var is set by the flutter test runner.
/// `false` in integration tests and run mode.
final bool _cfgTest = io.Platform.environment.containsKey("FLUTTER_TEST");

/// I'd really prefer to statically link `app-rs` everywhere, but
/// flutter_rust_bridge 2.0 currently makes this very difficult, since it no
/// longer dumps the actually-used symbols for anti-stripping mitigation.
///
/// So for now we're going dynamically linked everywhere...
ExternalLibrary _loadLibraryNormal() {
  if (io.Platform.isAndroid || io.Platform.isLinux) {
    return ExternalLibrary.open("libapp_rs.so");
  } else if (io.Platform.isIOS || io.Platform.isMacOS) {
    return ExternalLibrary.open("app_rs_dart.framework/app_rs_dart");
  } else if (io.Platform.isWindows) {
    return ExternalLibrary.open("app_rs_dart.dll");
  } else {
    throw UnsupportedError("Unknown platform: ${io.Platform.operatingSystem}");
  }
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
  } else if (io.Platform.isWindows) {
    // phlip9: untested
    return ExternalLibrary.open("../target/debug/libapp_rs.dll");
  } else {
    throw UnsupportedError(
      "Unsupported unit test platform: ${io.Platform.operatingSystem}",
    );
  }
}

/// Load the app-rs Rust FFI library.
ExternalLibrary _loadLibrary() {
  if (_cfgDebug && _cfgTest) {
    return _loadLibraryUnitTest();
  } else {
    return _loadLibraryNormal();
  }
}

/// The `app-rs` shared library that needs to be loaded at runtime.
final ExternalLibrary appRsLib = _loadLibrary();
