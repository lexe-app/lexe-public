import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show AppHandle, Config;
import 'package:lexeapp/logger.dart' as logger;
import 'package:lexeapp/logger.dart' show debug, error, info, trace, warn;

Future<void> main() async {
  final config = Config.regtest(bridge: api);

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(bridge: api, config: config), null);
  });

  test("logger", () async {
    logger.init();

    // dart logs
    trace("dart trace");
    debug("dart debug");
    info("dart info");
    warn("dart warn");
    error("dart error");

    // Need to let rustLogRx.listen register
    await Future.delayed(const Duration(milliseconds: 1));

    // make some Rust logs
    api.doLogs();

    // Need to flush Rust logs
    await Future.delayed(const Duration(milliseconds: 1));

    error("dart error");
  });
}
