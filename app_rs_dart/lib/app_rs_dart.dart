library;

import 'package:app_rs_dart/frb_generated.dart' show AppRs;
import 'package:app_rs_dart/load.dart' show appRsLib;

/// Initialize the native Rust ffi bindings. This must only be run once per
/// isolate.
Future<void> init() => AppRs.init(externalLibrary: appRsLib);
