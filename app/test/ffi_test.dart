import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/ffi.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show Config;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/cfg.dart' as cfg;

Future<void> main() async {
  final Config config = await cfg.buildTest();
  await app_rs_dart.init();

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(config: config), null);
  });
}
