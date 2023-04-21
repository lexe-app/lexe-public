import 'dart:io' show Platform;
import 'package:flutter/foundation.dart' show kDebugMode;

import 'bindings_generated_api.dart' show Config, DeployEnv, Network;

/// `true` when the flutter app is built in debug mode with debugging info and
/// debug symbols built-in (i.e., not profile or release mode).
///
/// Since this is a constant, the dart compiler can eliminate unreachable,
/// debug-only blocks.
const bool debug = kDebugMode;

/// `true` only in unit tests. This env var is set by the flutter test runner.
/// `false` in integration tests and run mode.
final bool test = Platform.environment.containsKey("FLUTTER_TEST");

const Config config = Config(
  deployEnv: DeployEnv.Dev,
  network: Network.Regtest,
  // TODO(phlip9): need different flavors for prod, staging, and dev
  gatewayUrl: String.fromEnvironment("DEV_GATEWAY_URL"),
  useSgx: false,
);

const Config testConfig = Config(
  deployEnv: DeployEnv.Dev,
  network: Network.Regtest,
  gatewayUrl: "",
  useSgx: false,
);
