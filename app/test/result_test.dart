// Ignore this lint as flutter_rust_bridge ffi errors don't impl Error/Exception...
// ignore_for_file: only_throw_errors

import 'package:app_rs_dart/app_rs_dart.dart' as app_rs_dart;
import 'package:app_rs_dart/ffi/debug.dart'
    show unconditionalError, unconditionalPanic;
import 'package:app_rs_dart/frb.dart' show PanicException;
import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/result.dart';

int conjure3() => 3;

int fakeApiSync(String param) {
  throw FfiError("Error $param").toFfi();
}

int fakeApiSync2() {
  throw const FfiError("Error").toFfi();
}

Future<int> fakeApiAsync(String param) async {
  throw FfiError("Error $param").toFfi();
}

Future<int> fakeApiAsync2() async {
  throw const FfiError("Error").toFfi();
}

void expectFirstLineEq(final String? actual, final String? expected) {
  expect(actual?.split('\n').firstOrNull, expected);
}

Future<void> main() async {
  await app_rs_dart.init();

  test("result : operator == and hashCode", () {
    const Result<int, void> ok1 = Ok(5);
    final int three = conjure3();
    final Result<int, void> ok2 = Ok(2 + three);
    final Result<int, void> ok3 = Ok(4 + three);
    final Result<int, void> ok4 = Ok(4 + three).map((x) => x - 2);

    expect(5, ok1.unwrap());
    expect(5, ok2.unwrap());
    expect(7, ok3.unwrap());
    expect(5, ok4.unwrap());

    assert(ok1 == ok2);
    assert(ok2 != ok3);
    assert(ok2 == ok4);

    assert(ok1.hashCode == ok2.hashCode);
    assert(ok1.hashCode != ok3.hashCode);
    assert(ok1.hashCode == ok4.hashCode);
  });

  test("result : tryFfi", () {
    final res1 = Result.tryFfi(() => fakeApiSync("foo"));
    expectFirstLineEq(res1.err?.message, "Error foo");

    final res2 = Result.tryFfi(fakeApiSync2);
    expectFirstLineEq(res2.err?.message, "Error");
  });

  test("result : tryFfiAsync", () async {
    final res1 = await Result.tryFfiAsync(() => fakeApiAsync("bar"));
    expectFirstLineEq(res1.err?.message, "Error bar");

    final res2 = await Result.tryFfiAsync(fakeApiAsync2);
    expectFirstLineEq(res2.err?.message, "Error");

    final res3 = await Result.tryFfiAsync(unconditionalError);
    expectFirstLineEq(res3.err?.message, "Error inside app-rs");
  });

  // The fake panic messages may include a stacktrace after the error message
  // when `RUST_BACKTRACE=1`, so we'll only compare the first line.

  test(
    "result : tryFfiAsync (panic)",
    skip: "panics always dump to stdout, cluttering test output",
    () async {
      try {
        final res1 = await Result.tryFfiAsync(unconditionalPanic);
        throw Exception("Panics should NOT be caught, res: $res1");
      } on PanicException catch (err) {
        expectFirstLineEq(err.message, "Panic inside app-rs");
      }
    },
  );
}
