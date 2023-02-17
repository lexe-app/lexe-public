import 'package:flutter_test/flutter_test.dart' show test, expect;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show Config;
import 'package:lexeapp/cfg.dart' as cfg;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  final config = Config.regtest(bridge: api);

  test("cfg.test is FALSE in integration tests", () {
    expect(cfg.test, false);
  });

  test("fresh app has no persisted state", () async {
    expect(await api.appLoad(config: config), false);
  });
}
