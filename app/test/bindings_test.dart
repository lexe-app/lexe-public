import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show AppHandle, Config;

void main() {
  final config = Config.regtest(bridge: api);

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(bridge: api, config: config), null);
  });
}
