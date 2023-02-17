import 'package:flutter_test/flutter_test.dart' show test, expect;

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show Config;

void main() {
  final config = Config.regtest(bridge: api);

  test("fresh app has no persisted state", () async {
    expect(await api.appLoad(config: config), false);
  });
}
