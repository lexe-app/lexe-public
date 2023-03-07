import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show AppHandle, Config;
import 'package:lexeapp/cfg.dart' as cfg;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  final config = Config.regtest(bridge: api);

  test("cfg.test is FALSE in integration tests", () {
    expect(cfg.test, false);
  });

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(bridge: api, config: config), null);
  });
}
