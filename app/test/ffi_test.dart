import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/app_rs/ffi/ffi.dart' show AppHandle, Config;
import 'package:lexeapp/app_rs/frb_generated.dart';
import 'package:lexeapp/app_rs/load.dart';
import 'package:lexeapp/cfg.dart' as cfg;

Future<void> main() async {
  final Config config = await cfg.buildTest();
  await AppRs.init(externalLibrary: appRsLib);

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(config: config), null);
  });
}
