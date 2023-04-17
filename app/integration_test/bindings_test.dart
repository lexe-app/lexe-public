import 'package:flutter_rust_bridge/flutter_rust_bridge.dart' show FfiException;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show AppHandle, Config;
import 'package:lexeapp/cfg.dart' as cfg;

void assertFfiExceptionMsgHasBacktrace(FfiException err) {
  assert(
    err.message.contains("Stack backtrace") ||
        err.message.contains("stack backtrace"),
    "FfiException doesn't contain backtrace: err:\n${err.message}\n",
  );
  assert(
    !err.message.contains("0: <unknown>"),
    "FfiException backtrace isn't symbolized properly:\n${err.message}\n",
  );
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  final config = Config.regtest(bridge: api);

  test("cfg.test is FALSE in integration tests", () {
    expect(cfg.test, false);
  });

  test("fresh app has no persisted state", () async {
    expect(await AppHandle.load(bridge: api, config: config), null);
  });

  // Ensure we're getting properly symbolized backtraces from panics and errors
  // created in the rust FFI.

  test(
    "panic sync in rust ffi should give backtrace",
    () {
      try {
        api.doPanicSync();
      } on FfiException catch (err) {
        assertFfiExceptionMsgHasBacktrace(err);
      }
    },
    skip: "TODO: panics are not giving backtraces at all...",
  );

  test(
    "panic async in rust ffi should give backtrace",
    () async {
      try {
        await api.doPanicAsync();
      } on FfiException catch (err) {
        assertFfiExceptionMsgHasBacktrace(err);
      }
    },
    skip: "TODO: panics are not giving backtraces at all...",
  );

  test(
    "result err from rust ffi should give backtrace",
    () {
      try {
        api.doReturnErrSync();
      } on FfiException catch (err) {
        assertFfiExceptionMsgHasBacktrace(err);
      }
    },
    skip: "TODO: backtraces aren't symbolized",
  );

  test(
    "result err from rust ffi should give backtrace",
    () async {
      try {
        await api.doReturnErrAsync();
      } on FfiException catch (err) {
        assertFfiExceptionMsgHasBacktrace(err);
      }
    },
    skip: "TODO: backtraces aren't symbolized",
  );
}
