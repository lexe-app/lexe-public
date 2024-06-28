import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/ffi/ffi.dart' show api;
import 'package:lexeapp/ffi/ffi_generated_api.dart' show AppHandle, Config;

Future<void> main() async {
  final Config config = await cfg.buildTest();

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(bridge: api, config: config), null);
  });
}
