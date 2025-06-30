import 'dart:math' show Random;

import 'package:flutter_test/flutter_test.dart' show fail, test;
import 'package:lexeapp/string_ext.dart' show StringExt;

Future<void> main() async {
  test("StringExt.countLines", () {
    final rng = Random(202502251228);
    for (int idx = 0; idx < 1000; idx += 1) {
      final s = rng.genString();
      final actual = s.countLines();
      final expect = countLinesOracle(s);
      if (actual != expect) {
        fail(
          "expect: $expect\nactual: $actual\n input: \"$s\"\n codes: ${s.codeUnits}",
        );
      }
    }
  });
}

int countLinesOracle(final String s) => s.split('\n').length;

extension RandomExt on Random {
  String genString() {
    final len = this.nextInt(1024);
    final utf16 = List.generate(
      len,
      (_idx) => this.genUtf16Code(),
      growable: false,
    );
    return String.fromCharCodes(utf16);
  }

  int genUtf16Code() {
    final p = this.nextDouble();

    // ASCII printable characters
    if (p <= 0.5) return this.nextInt(0x7f - 0x20) + 0x20;
    // newline (\n)
    if (p <= 0.6) return 0x0a;

    // all other unicode
    return this.nextInt(0x10000);
  }
}
