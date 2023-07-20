import 'dart:io' show Directory, Platform;
import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:path_provider/path_provider.dart' as path_provider;

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

Future<Config> build() async {
  // Application Support is for app-specific data that is not meant to be
  // user-facing, unlike `path_provider.getApplicationDocumentsDirectory()`.
  // On Android, iOS, and macOS, this data is also sandboxed and inaccessible
  // to other apps.
  //
  // This is also not the fully qualified data dir. We need to disambiguate b/w
  // (dev/staging/prod) x (regtest/testnet/mainnet) x (sgx/dbg).
  // See: `app-rs::app::AppConfig`
  final baseAppDataDir = await path_provider.getApplicationSupportDirectory();

  return Config(
    deployEnv: DeployEnv.Dev,
    network: Network.Regtest,
    gatewayUrl: const String.fromEnvironment("DEV_GATEWAY_URL"),
    useSgx: _useSgx,
    baseAppDataDir: baseAppDataDir.path,
    useMockSecretStore: false,
  );
}

Future<Config> buildTest() async {
  // Use a temp dir for unit tests, since `path_provider` doesn't work in tests.
  final baseAppDataDir = await Directory.systemTemp.createTemp("lexeapp");

  return Config(
    deployEnv: DeployEnv.Dev,
    network: Network.Regtest,
    gatewayUrl: "",
    useSgx: false,
    baseAppDataDir: baseAppDataDir.path,
    useMockSecretStore: true,
  );
}
