import 'package:flutter_test/flutter_test.dart' show test, expect;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/cfg.dart' as cfg;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  test("cfg.test is FALSE in integration tests", () {
    expect(cfg.test, false);
  });

  test("bindings work", () {
    expect(api.hello(), "hello!");
  });
}
