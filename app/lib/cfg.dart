//! # Lexe app build config
//!
//! ### Build-time Environment Variables:
//!
//! * `DEPLOY_ENVIRONMENT`: prod, staging, dev
//! * `NETWORK`: mainnet, testnet3, testnet4, regtest
//! * `SGX`: true, false (whether the app should expect SGX nodes)
//! * `RUST_LOG`: e.g. "app-rs=trace,http=debug,warn"
//! * `DEV_GATEWAY_URL`: url of local development gateway

import 'dart:io' show Directory, Platform;

import 'package:app_rs_dart/ffi/types.dart' show Config, DeployEnv, Network;
import 'package:flutter/foundation.dart' show kDebugMode, kReleaseMode;
import 'package:flutter/services.dart' show appFlavor;
import 'package:path_provider/path_provider.dart' as path_provider;

/// `true` when the flutter app is built in debug mode with debugging info and
/// debug symbols built-in (i.e., not profile or release mode).
///
/// Since this is a constant, the dart compiler can eliminate unreachable,
/// debug-only blocks.
const bool debug = kDebugMode;

/// `true` when the flutter is built for release (i.e., not debug or profile).
const bool release = kReleaseMode;

/// App flavor.
const bool design = appFlavor == "design";
const bool dev = appFlavor == null || appFlavor == "dev";
const bool staging = appFlavor == "staging";
const bool prod = appFlavor == "prod";

/// `true` only in unit tests. This env var is set by the flutter test runner.
/// `false` in integration tests and run mode.
final bool test = Platform.environment.containsKey("FLUTTER_TEST");

// Environment variables that control the build variant.
// NOTE: we need default values otherwise the dart LSP constantly complains...

// env $DEPLOY_ENVIRONMENT
const String _deployEnvStr =
    String.fromEnvironment("DEPLOY_ENVIRONMENT", defaultValue: "dev");
// This call should never fail after the compile-time checks below.
final DeployEnv deployEnv = DeployEnv.fromStr(s: _deployEnvStr);

// env $NETWORK
const String _networkStr =
    String.fromEnvironment("NETWORK", defaultValue: "regtest");
// This call should never fail after the compile-time checks below.
final Network network = Network.fromStr(s: _networkStr);

// env $SGX
const String _useSgxStr = String.fromEnvironment("SGX", defaultValue: "false");
const bool _useSgx = _useSgxStr == "true";

// env $RUST_LOG
const String _rustLogStr =
    String.fromEnvironment("RUST_LOG", defaultValue: "info");

// TODO(phlip9): need a more production-ready way to configure this
const String _devGatewayUrlStr = String.fromEnvironment(
  "DEV_GATEWAY_URL",
  defaultValue: "https://127.0.0.1:4040",
);

// build configuration values implied by the current `--flavor=<appFlavor>`

// The expected `DeployEnv` value for a given `--flavor=<appFlavor>`.
// We'll assert on this down below.
const String _flavorDeployEnvStr =
    (dev || design) ? "dev" : (staging ? "staging" : (prod ? "prod" : "ERROR"));

// The expected `Network` value for a given `--flavor=<appFlavor>`.
// We'll assert on this down below.
const String _flavorNetworkStr = (dev || design)
    ? _networkStr
    : (staging ? "testnet3" : (prod ? "mainnet" : "ERROR"));

// The expected `useSgx` value for a given `--flavor=<appFlavor>`.
// We'll assert on this down below.
const bool _flavorUseSgx =
    (dev || design) ? _useSgx : ((staging || prod) ? true : false);

// Compile-time assertions so we can throw a compile error if these somehow get
// misconfigured.

class _AssertDeployEnv {
  const _AssertDeployEnv(String s)
      : assert(s == "prod" || s == "staging" || s == "dev");
}

class _AssertNetworkEnv {
  const _AssertNetworkEnv(String s)
      : assert(s == "mainnet" ||
            s == "testnet3" ||
            s == "testnet4" ||
            s == "regtest");
}

class _AssertBoolEnv {
  const _AssertBoolEnv(String s) : assert(s == "true" || s == "false");
}

class _AssertAppFlavor {
  const _AssertAppFlavor(
    String envDeployEnv,
    String envNetwork,
    bool envUseSgx,
    String flavorDeployEnv,
    String flavorNetwork,
    bool flavorUseSgx,
  ) : assert(
          envDeployEnv == flavorDeployEnv &&
              envNetwork == flavorNetwork &&
              envUseSgx == flavorUseSgx,
        );
}

// ignore: unused_element,  constant_identifier_names
const _1 = _AssertDeployEnv(_deployEnvStr);
// ignore: unused_element, constant_identifier_names
const _2 = _AssertNetworkEnv(_networkStr);
// ignore: unused_element, constant_identifier_names
const _3 = _AssertBoolEnv(_useSgxStr);
// ignore: unused_element, constant_identifier_names
const _4 = _AssertAppFlavor(
  _deployEnvStr,
  _networkStr,
  _useSgx,
  _flavorDeployEnvStr,
  _flavorNetworkStr,
  _flavorUseSgx,
);

/// Build a [Config] that will actually talk to the lexe backend. That could be
/// the real production backend or just a local development version.
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

  final gatewayUrl = switch (deployEnv) {
    DeployEnv.prod => "https://lexe-prod.uswest2.prod.lexe.app",
    DeployEnv.staging => "https://lexe-staging-sgx.uswest2.staging.lexe.app",
    // Use the build-time env $DEV_GATEWAY_URL in local dev.
    // We can't hard code this since deploying to a real mobile device in dev
    // requires connecting to the dev machine over the local LAN.
    DeployEnv.dev => _devGatewayUrlStr,
  };

  return Config(
    deployEnv: deployEnv,
    network: network,
    useSgx: _useSgx,
    gatewayUrl: gatewayUrl,
    baseAppDataDir: baseAppDataDir.path,
    useMockSecretStore: false,
  );
}

/// Build a [Config] suitable for unit tests or UI design mode.
Future<Config> buildTest() async {
  // Use a temp dir for unit tests, since `path_provider` doesn't work in tests.
  final baseAppDataDir = await Directory.systemTemp.createTemp("lexeapp");

  return Config(
    deployEnv: DeployEnv.dev,
    network: Network.regtest,
    useSgx: false,
    gatewayUrl: "<no-dev-gateway-url>",
    baseAppDataDir: baseAppDataDir.path,
    useMockSecretStore: true,
  );
}

// Load the log filter from the environment. Priority:
// 1. runtime env: `$RUST_LOG` (NOTE: not easily available on mobile!)
// 2. build-time env: `flutter run --dart-define=RUST_LOG=$RUST_LOG ..`
//    (for `String.fromEnvironment`, for mobile)
// 3. default: "info"
String rustLogFromEnv() {
  final String? runtimeRustLog = Platform.environment["RUST_LOG"];

  if (runtimeRustLog != null) {
    return runtimeRustLog;
  }

  return _rustLogStr;
}
