import 'dart:io' show Directory, Platform;

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:flutter_test/flutter_test.dart'
    show endsWith, expect, fail, isTrue, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:package_info_plus/package_info_plus.dart' show PackageInfo;

void main() async {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  await app_rs_dart.init();

  // Wallet data lives here. If this ever moves, existing users lose access
  // to their wallet on upgrade.
  test("cfg.build lexeDataDir is stable", () async {
    final config = await cfg.build(cfg.UserAgent.dummy());
    await config.validate();

    final lexeDataDir = config.lexeDataDir;
    expect(Directory(lexeDataDir).isAbsolute, isTrue);
    expect(await Directory(lexeDataDir).exists(), isTrue);

    // Snapshot lexeDataDir (well, the suffix at least, since the prefix is
    // usually not stable).
    final appId = (await PackageInfo.fromPlatform()).packageName;
    final expectedSuffix = switch (Platform.operatingSystem) {
      "android" => "/$appId/files",
      "ios" => "/Library/Application Support",
      "macos" =>
        "/Library/Containers/$appId/Data/Library/Application Support/$appId",
      final os => fail("unexpected platform: $os"),
    };
    expect(lexeDataDir, endsWith(expectedSuffix));
  });
}
