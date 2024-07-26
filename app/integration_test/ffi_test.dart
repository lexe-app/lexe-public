import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;
import 'package:lexeapp/cfg.dart' as cfg;

void main() async {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  await app_rs_dart.init();

  final config = await cfg.buildTest();

  test("cfg.test is FALSE in integration tests", () {
    expect(cfg.test, false);
  });

  test("fresh app has no persisted state", () async {
    final maybeApp = await AppHandle.load(config: config);
    expect(maybeApp, null);
  });
}
