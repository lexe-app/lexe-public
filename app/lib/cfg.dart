import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kDebugMode;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show Config, DeployEnv, Network;

/// `true` when the flutter app is built in debug mode with debugging info and
/// debug symbols built-in (i.e., not profile or release mode).
///
/// Since this is a constant, the dart compiler can eliminate unreachable,
/// debug-only blocks.
const bool debug = kDebugMode;

/// `true` only in unit tests.
/// `false` in integration tests and run mode.
final bool test = Platform.environment.containsKey("FLUTTER_TEST");

// TODO(phlip9): make this configurable via
/// [flutter build flavors](https://docs.flutter.dev/deployment/flavors)
final Config config = Config(
  bridge: api,
  deployEnv: DeployEnv.Dev,
  network: Network.Regtest,
);
